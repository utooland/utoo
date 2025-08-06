use anyhow::Result;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::OnceLock;

// Global CWD static variable accessible to all modules
static CWD: OnceLock<Mutex<String>> = OnceLock::new();

/// Directory entry with name and type information
#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_dir: bool,
}

pub mod fuse;
pub mod model;
pub mod package_manager;
pub mod util;

pub mod opfs_fs {
    use super::*;

    /// Read file content with fuse.link support
    pub async fn read(path: &str) -> Result<Vec<u8>> {
        fuse::read(path).await
    }

    /// Read file content as bytes (without fuse.link support)
    pub async fn read_bytes(path: &str) -> Result<Vec<u8>> {
        let prepared_path = crate::util::prepare_path(path).await?;
        let content = tokio_fs_ext::read(&prepared_path).await?;
        Ok(content)
    }

    /// Read directory contents with file type information and fuse.link support
    pub async fn read_dir(path: &str) -> Result<Vec<DirEntry>> {
        fuse::read_dir(path).await
    }

    /// Write content to file
    pub async fn write(path: &str, content: &str) -> Result<()> {
        // to buffer
        let buffer = content.as_bytes();
        tokio_fs_ext::write(path, buffer)
            .await
            .map_err(|e| anyhow::anyhow!("write error: {e}"))?;
        Ok(())
    }

    /// Write binary content to file
    pub async fn write_bytes(path: &str, content: &[u8]) -> Result<()> {
        tokio_fs_ext::write(path, content)
            .await
            .map_err(|e| anyhow::anyhow!("write_bytes error: {e}"))?;
        Ok(())
    }

    pub async fn create_dir_all(path: &str) -> Result<()> {
        tokio_fs_ext::create_dir_all(path)
            .await
            .map_err(|e| anyhow::anyhow!("create_dir_all error: {e}"))?;
        Ok(())
    }

    /// Remove a file
    pub async fn remove(path: &str) -> Result<()> {
        tokio_fs_ext::remove_file(path).await?;
        Ok(())
    }

    /// Create directory (including parent directories)
    pub async fn write_dir(path: &str) -> Result<()> {
        tokio_fs_ext::create_dir_all(path).await?;
        Ok(())
    }

    /// Remove directory and its contents
    pub async fn remove_dir(path: &str) -> Result<()> {
        tokio_fs_ext::remove_dir_all(path).await?;
        Ok(())
    }

    /// Remove directory and its contents
    pub async fn copy(src: &str, dst: &str) -> Result<()> {
        tokio_fs_ext::copy(src, dst).await?;
        Ok(())
    }

    /// Get canonical path
    pub async fn canonicalize(path: &str) -> Result<String> {
        let canonical_path = tokio_fs_ext::canonicalize(path).await?;
        if let Some(path_str) = canonical_path.to_str() {
            Ok(path_str.to_string())
        } else {
            Err(anyhow::anyhow!("Invalid path encoding"))
        }
    }

    /// Check if file or directory exists
    pub async fn exists(path: &str) -> Result<bool> {
        match tokio_fs_ext::metadata(path).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

pub mod cwd {
    use super::*;

    // FIXME: This is not thread-safe, we need to use a thread-local variable instead
    /// Set current working directory
    pub async fn set_cwd(path: &str) -> Result<()> {
        if let Some(cwd) = CWD.get() {
            let mut guard = cwd.lock().unwrap();
            *guard = path.to_string();
        } else {
            CWD.get_or_init(|| {
                // FXIME: cwd should be set by the caller
                Mutex::new("/utoo-wasm-demo".to_string())
            });
        }
        Ok(())
    }

    /// Read current working directory
    pub async fn get_cwd() -> Result<String> {
        if let Some(cwd) = CWD.get() {
            let current_cwd = cwd.lock().unwrap().clone();
            Ok(current_cwd)
        } else {
            let cwd = CWD.get_or_init(|| {
                // FXIME: cwd should be set by the caller
                Mutex::new(String::from("/utoo-wasm-demo"))
            });
            let current_cwd = cwd.lock().unwrap().clone();
            Ok(current_cwd)
        }
    }
}
