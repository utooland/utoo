use anyhow::{bail, Context, Result};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::{env, fs};

pub fn link(src: &Path, dst: &Path) -> Result<()> {
    // get current working directory as prefix
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    // ensure the destination directory exists
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).context(format!(
            "Failed to create parent directory: {}",
            parent.display()
        ))?;
    }

    let abs_src = cwd.join(src);
    let abs_dst = cwd.join(dst);

    // Check if source exists
    if !abs_src.exists() {
        bail!("Source file does not exist: {}", abs_src.display());
    }

    // Check if destination exists or is a broken symlink
    if fs::symlink_metadata(&abs_dst).is_ok() {
        fs::remove_file(&abs_dst).context(format!(
            "Failed to remove existing file: {}",
            abs_dst.display()
        ))?;
    }

    symlink(&abs_src, &abs_dst).context(format!(
        "Failed to create symbolic link from {} to {}",
        abs_src.display(),
        abs_dst.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_link_creates_new_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_content = "test content";
        let src_path = temp_path.join("source1.txt");
        fs::write(&src_path, src_content).unwrap();

        let dst_path = temp_path.join("dest1.txt");

        assert!(!dst_path.exists());
        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).unwrap();

        assert!(dst_path.exists());
        assert!(dst_path.is_symlink());
        assert_eq!(fs::read_to_string(&dst_path).unwrap(), src_content);
    }

    #[test]
    fn test_link_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source2.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("nested/dir/dest2.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).unwrap();

        assert!(dst_path.exists());
        assert!(dst_path.is_symlink());
    }

    #[test]
    fn test_link_existing_same_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source3.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("dest3.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).unwrap();
        let result = link(&src_path, &dst_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_link_existing_different_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src1_path = temp_path.join("source4a.txt");
        let src2_path = temp_path.join("source4b.txt");
        fs::write(&src1_path, "test1").unwrap();
        fs::write(&src2_path, "test2").unwrap();

        let dst_path = temp_path.join("dest4.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src1_path, &dst_path).unwrap();
        let result = link(&src2_path, &dst_path);
        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(&dst_path).unwrap(), "test2");
    }
}
