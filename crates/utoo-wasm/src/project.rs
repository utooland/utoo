//! WASM API exports for Project operations.
//!
//! This module only contains wasm_bindgen API bindings.
//! Core implementations are in:
//! - `pack` module for build/dev operations
//! - `pm` module for package manager operations (deps, install, gzip, md5)

use parking_lot::RwLock;
use wasm_bindgen::prelude::*;

use crate::errors::to_js_error;
use crate::pm;
use crate::tokio_runtime::init_tokio_runtime;

#[cfg(feature = "utoopack")]
use crate::pack;

static GLOBAL_THREAD_URL: RwLock<Option<String>> = RwLock::new(None);

#[wasm_bindgen]
pub struct Project;

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(js_name = init)]
    pub fn init(thread_url: String) {
        let mut url_guard = GLOBAL_THREAD_URL.write();
        if url_guard.is_none() && !thread_url.is_empty() {
            *url_guard = Some(thread_url.clone());
        }
        let final_url = url_guard.as_ref().cloned().unwrap_or(thread_url);
        drop(url_guard);

        if !final_url.is_empty() {
            init_tokio_runtime(final_url);
        }
    }

    #[wasm_bindgen(js_name = setCwd)]
    pub fn set_cwd(path: String) {
        opfs_project::set_cwd(path);
    }

    #[wasm_bindgen(getter)]
    pub fn cwd() -> String {
        opfs_project::get_cwd().to_string_lossy().to_string()
    }

    /// Calculate MD5 hash of byte content (async for better thread scheduling)
    #[wasm_bindgen(js_name = sigMd5)]
    pub async fn sig_md5(content: js_sys::Uint8Array) -> Result<String, JsError> {
        pm::sig_md5(content.to_vec()).await.map_err(to_js_error)
    }

    /// Create a tar.gz archive and return bytes (no file I/O)
    #[wasm_bindgen(js_name = gzip)]
    pub async fn gzip(files: JsValue) -> Result<js_sys::Uint8Array, JsError> {
        let bytes = pm::gzip(files).await.map_err(to_js_error)?;
        Ok(js_sys::Uint8Array::from(&bytes[..]))
    }

    /// Generate package-lock.json by resolving dependencies.
    #[wasm_bindgen]
    pub async fn deps(
        registry: Option<String>,
        concurrency: Option<usize>,
    ) -> Result<String, String> {
        pm::deps(registry.as_deref(), concurrency)
            .await
            .map_err(|e| format!("{:#?}", e))
    }

    /// Install dependencies - downloads tgz files only, extracts on-demand when files are read
    #[wasm_bindgen]
    pub async fn install(
        package_lock: String,
        max_concurrent_downloads: Option<usize>,
    ) -> Result<(), JsError> {
        let concurrency = max_concurrent_downloads.unwrap_or(20);
        pm::install(&package_lock, concurrency)
            .await
            .map_err(to_js_error)
    }

    #[cfg(feature = "utoopack")]
    #[wasm_bindgen]
    pub async fn build() -> Result<JsValue, JsError> {
        pack::build().await
    }

    #[cfg(feature = "utoopack")]
    #[wasm_bindgen]
    pub async fn dev() -> Result<JsValue, JsError> {
        pack::dev().await
    }

    #[cfg(not(feature = "utoopack"))]
    #[wasm_bindgen]
    pub async fn build() -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Build functionality requires the 'utoopack' feature to be enabled",
        ))
    }

    #[cfg(not(feature = "utoopack"))]
    #[wasm_bindgen]
    pub async fn dev() -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Dev functionality requires the 'utoopack' feature to be enabled",
        ))
    }
}
