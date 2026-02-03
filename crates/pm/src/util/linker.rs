use crate::fs;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::instrument;

/// Convert a path to absolute path
fn to_absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().context("Failed to get current working directory")?;
        Ok(cwd.join(path))
    }
}

#[instrument(name = "link", skip_all)]
pub async fn link(src: &Path, dst: &Path) -> Result<()> {
    // Convert to absolute paths
    let abs_src = to_absolute(src)?;
    let abs_dst = to_absolute(dst)?;

    // Check if source exists
    if !fs::try_exists(&abs_src).await? {
        anyhow::bail!("Source file does not exist: {}", abs_src.display());
    }

    // Ensure the destination directory exists
    if let Some(parent) = abs_dst.parent() {
        fs::create_dir_all(parent).await.context(format!(
            "Failed to create parent directory: {}",
            parent.display()
        ))?;
    }

    // Check if destination already exists
    if let Ok(metadata) = fs::symlink_metadata(&abs_dst).await {
        // If it's already a symlink pointing to the correct source, nothing to do
        if metadata.is_symlink()
            && let Ok(target) = fs::read_link(&abs_dst).await
            && target == abs_src
        {
            return Ok(());
        }

        // Remove existing file/symlink/directory (like ln -sf)
        if metadata.is_dir() {
            fs::remove_dir_all(&abs_dst).await
        } else {
            fs::remove_file(&abs_dst).await
        }
        .context(format!(
            "Failed to remove existing path: {}",
            abs_dst.display()
        ))?;
    }

    // Create symlink based on platform
    create_symlink(&abs_src, &abs_dst).await?;

    Ok(())
}

/// Create a symlink (cross-platform)
#[cfg(unix)]
async fn create_symlink(src: &Path, dst: &Path) -> Result<()> {
    fs::symlink(src, dst).await.context(format!(
        "Failed to create symbolic link from {} to {}",
        src.display(),
        dst.display()
    ))
}

#[cfg(windows)]
async fn create_symlink(src: &Path, dst: &Path) -> Result<()> {
    // On Windows, we need to distinguish between file and directory symlinks
    if fs::metadata(src).await?.is_dir() {
        fs::symlink_dir(src, dst).await
    } else {
        fs::symlink_file(src, dst).await
    }
    .context(format!(
        "Failed to create symbolic link from {} to {}",
        src.display(),
        dst.display()
    ))
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

        #[cfg(unix)]
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

        #[cfg(unix)]
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

    #[tokio::test]
    async fn test_link_replaces_existing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source5.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("dest5_dir");
        // Create a directory at destination
        fs::create_dir_all(&dst_path).unwrap();
        fs::write(dst_path.join("file.txt"), "content").unwrap();

        env::set_current_dir(temp_path).unwrap();
        let result = link(&src_path, &dst_path).await;
        assert!(result.is_ok());

        #[cfg(unix)]
        assert!(dst_path.is_symlink());

        assert_eq!(fs::read_to_string(&dst_path).unwrap(), "test");
    }
}
