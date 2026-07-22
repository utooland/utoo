//! Package manager related operations.
//!
//! This module contains implementations for:
//! - MD5 signature calculation
//! - Gzip archive creation
//! - Dependency resolution
//! - Package installation
//! - Global OpfsProject instance management

use anyhow::{anyhow, Result};
use opfs_project::archive::PackFile;
use parking_lot::RwLock;
use std::{path::Path, sync::Arc};
use wasm_bindgen::JsCast;

use crate::tokio_runtime::{runtime, TOKIO_RUNTIME};

/// Global OpfsProject instance, initialised the first time `Project::init` is called.
pub(crate) static OPFS_PROJECT: RwLock<Option<Arc<opfs_project::OpfsProject>>> = RwLock::new(None);

/// Get a reference to the global OpfsProject for reading.
///
/// Panics if the project has not been initialised via `Project::init`.
pub(crate) fn with_project<R>(f: impl FnOnce(&opfs_project::OpfsProject) -> R) -> R {
    let guard = OPFS_PROJECT.read();
    let project = guard
        .as_ref()
        .expect("OpfsProject not initialised — call Project.init() first");
    f(project)
}

/// Calculate MD5 hash of byte content
pub async fn sig_md5(content: Vec<u8>) -> Result<String> {
    let result = runtime()
        .spawn_blocking(move || opfs_project::archive::sig_md5(&content))
        .await?;
    Ok(result)
}

/// Create a tar.gz archive from a list of files (internal)
async fn gzip_files(pack_files: Vec<PackFile>) -> Result<Vec<u8>> {
    let bytes = runtime()
        .spawn_blocking(move || opfs_project::archive::gzip(&pack_files))
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
    let cwd = with_project(|p| p.cwd().to_path_buf());

    let package_lock =
        crate::deps::build_deps_from_file(Path::new(&cwd), registry, concurrency).await?;

    serde_json::to_string_pretty(&package_lock)
        .map_err(|e| anyhow!("Failed to serialize package lock: {}", e))
}

/// Install dependencies - downloads tgz files only, extracts on-demand when files are read
pub async fn install(
    package_lock_json: &str,
    max_concurrent_downloads: Option<usize>,
) -> Result<()> {
    let lock = opfs_project::package_lock::PackageLock::from_json(package_lock_json)
        .map_err(|e| anyhow!("Failed to parse package-lock.json: {}", e))?;

    let opts = opfs_project::InstallOptions {
        max_concurrent_downloads,
        omit: vec![],
    };

    let project = OPFS_PROJECT
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow!("OpfsProject not initialised"))?;

    project
        .install(&lock, &opts)
        .await
        .map_err(|e| anyhow!("{}", e))?;

    Ok(())
}
