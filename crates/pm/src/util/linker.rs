use anyhow::{Context, Result};
use std::path::Path;
use tokio::fs;

pub async fn link(src: &Path, dst: &Path) -> Result<()> {
    // Canonicalize source path (must exist)
    let abs_src = std::fs::canonicalize(src)
        .context(format!("Source file does not exist: {}", src.display()))?;

    // Convert destination to absolute path
    let abs_dst = if dst.is_absolute() {
        dst.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to get current working directory")?
            .join(dst)
    };

    // Ensure the destination directory exists
    if let Some(parent) = abs_dst.parent() {
        fs::create_dir_all(parent).await.context(format!(
            "Failed to create parent directory: {}",
            parent.display()
        ))?;
    }

    // Check if destination exists or is a broken symlink
    if fs::symlink_metadata(&abs_dst).await.is_ok() {
        // Check if it's already pointing to the correct source
        if let Ok(target) = fs::read_link(&abs_dst).await
            && target == abs_src {
                // Already correctly linked, nothing to do
                return Ok(());
            }

        // Remove existing file/symlink
        fs::remove_file(&abs_dst).await.context(format!(
            "Failed to remove existing file: {}",
            abs_dst.display()
        ))?;
    }

    // Create the symlink
    fs::symlink(&abs_src, &abs_dst).await.context(format!(
        "Failed to create symbolic link from {} to {}",
        abs_src.display(),
        abs_dst.display()
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_link_creates_new_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_content = "test content";
        let src_path = temp_path.join("source1.txt");
        fs::write(&src_path, src_content).unwrap();

        let dst_path = temp_path.join("dest1.txt");

        assert!(!dst_path.exists());
        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).await.unwrap();

        assert!(dst_path.exists());
        assert!(dst_path.is_symlink());
        assert_eq!(fs::read_to_string(&dst_path).unwrap(), src_content);
    }

    #[tokio::test]
    async fn test_link_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source2.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("nested/dir/dest2.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).await.unwrap();

        assert!(dst_path.exists());
        assert!(dst_path.is_symlink());
    }

    #[tokio::test]
    async fn test_link_existing_same_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source3.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("dest3.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src_path, &dst_path).await.unwrap();
        let result = link(&src_path, &dst_path);
        assert!(result.await.is_ok());
    }

    #[tokio::test]
    async fn test_link_existing_different_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src1_path = temp_path.join("source4a.txt");
        let src2_path = temp_path.join("source4b.txt");
        fs::write(&src1_path, "test1").unwrap();
        fs::write(&src2_path, "test2").unwrap();

        let dst_path = temp_path.join("dest4.txt");

        env::set_current_dir(temp_path).unwrap();
        link(&src1_path, &dst_path).await.unwrap();
        let result = link(&src2_path, &dst_path);
        assert!(result.await.is_ok());
        assert_eq!(fs::read_to_string(&dst_path).unwrap(), "test2");
    }
}
