//! Package manager related operations.
//!
//! This module contains implementations for:
//! - MD5 signature calculation
//! - Gzip archive creation
//! - Dependency resolution
//! - Package installation

use anyhow::{anyhow, Result};
use opfs_project::pack::PackFile;
use std::path::Path;
use wasm_bindgen::JsCast;

use crate::tokio_runtime::TOKIO_RUNTIME;

/// Calculate MD5 hash of byte content
pub async fn sig_md5(content: Vec<u8>) -> Result<String> {
    let result = runtime()
        .spawn_blocking(move || opfs_project::pack::sig_md5(&content))
        .await?;

    let result = rt
        .spawn_blocking(move || opfs_project::pack::sig_md5(&content))
        .await?;
    Ok(result)
}

/// Create a tar.gz archive from a list of files (internal)
async fn gzip_files(pack_files: Vec<PackFile>) -> Result<Vec<u8>> {
    let rt = TOKIO_RUNTIME
        .get()
        .ok_or_else(|| anyhow!("tokio runtime not initialized"))?;

    let bytes = rt
        .spawn_blocking(move || opfs_project::pack::gzip(&pack_files))
        .await??;

    Ok(bytes)
}

/// Create a tar.gz archive from JsValue array of {path, content} objects
pub async fn gzip(files: wasm_bindgen::JsValue) -> Result<Vec<u8>> {
    let files_array: js_sys::Array = files
        .dyn_into()
        .map_err(|e| anyhow!("files must be an array: {:?}", e))?;

    let mut pack_files: Vec<PackFile> = Vec::with_capacity(files_array.length() as usize);

    for i in 0..files_array.length() {
        let item = files_array.get(i);
        let path = js_sys::Reflect::get(&item, &"path".into())
            .map_err(|e| anyhow!("missing path at index {}: {:?}", i, e))?
            .as_string()
            .ok_or_else(|| anyhow!("path must be a string at index {}", i))?;
        let content_js = js_sys::Reflect::get(&item, &"content".into())
            .map_err(|e| anyhow!("missing content at index {}: {:?}", i, e))?;
        let content_arr: js_sys::Uint8Array = content_js
            .dyn_into()
            .map_err(|e| anyhow!("content must be Uint8Array at index {}: {:?}", i, e))?;
        pack_files.push(PackFile::new(path, content_arr.to_vec()));
    }

    gzip_files(pack_files).await
}

/// Generate package-lock.json by resolving dependencies
pub async fn deps(registry: Option<&str>, concurrency: Option<usize>) -> Result<String> {
    let cwd = opfs_project::get_cwd();
    let package_lock =
        crate::deps::build_deps_from_file(Path::new(&cwd), registry, concurrency).await?;

    // Serialize to JSON string
    serde_json::to_string_pretty(&package_lock)
        .map_err(|e| anyhow!("Failed to serialize package lock: {}", e))
}

/// Install dependencies - downloads tgz files only, extracts on-demand when files are read
pub async fn install(package_lock: &str, concurrency: usize) -> Result<()> {
    opfs_project::package_manager::install_deps(package_lock, concurrency).await?;
    Ok(())
}
