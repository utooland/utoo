#![cfg(all(target_family = "wasm", target_os = "unknown"))]

use anyhow::Result;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::OnceLock;
use wasm_bindgen::prelude::wasm_bindgen;

// Global CWD static variable accessible to all modules
// Maybe should not global in the future
static CWD: OnceLock<Mutex<String>> = OnceLock::new();

/// Directory entry with name and type information
#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    #[wasm_bindgen(getter_with_clone)]
    pub name: String,
    #[wasm_bindgen]
    pub r#type: DirEntryType,
}

#[wasm_bindgen]
#[derive(Debug, Copy, Clone)]
pub enum DirEntryType {
    File = "file",
    Directory = "directory",
}

mod fuse;
mod package_lock;
pub mod package_manager;
mod util;

pub mod opfs {
    use super::*;

    /// Read file content with fuse.link support
    pub async fn read_with_fuse_link(path: &str) -> Result<Vec<u8>> {
        fuse::read(path).await
    }

    /// Read file content as bytes (without fuse.link support)
    pub(crate) async fn read_without_fuse_link(path: &str) -> Result<Vec<u8>> {
        let prepared_path = crate::util::prepare_path(path);
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

    pub async fn create_dir(path: &str) -> Result<()> {
        tokio_fs_ext::create_dir(path).await?;
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

    /// Set current working directory
    pub fn set_cwd(path: String) {
        if let Some(cwd) = CWD.get() {
            let mut guard = cwd.lock().unwrap();
            *guard = path.to_string();
        } else {
            CWD.get_or_init(|| Mutex::new(path));
        }
    }

    /// Read current working directory
    pub fn get_cwd() -> String {
        if let Some(cwd) = CWD.get() {
            cwd.lock().unwrap().clone()
        } else {
            let cwd = CWD.get_or_init(|| Mutex::new(String::from("/")));
            cwd.lock().unwrap().clone()
        }
    }
}
