//! WASM API exports for Project operations.
//!
//! This module only contains wasm_bindgen API bindings.
//! Core implementations are in:
//! - `pack` module for build/dev operations
//! - `pm` module for package manager operations (deps, install, gzip, md5)

use std::sync::Arc;

use parking_lot::RwLock;
use wasm_bindgen::prelude::*;

use crate::errors::to_js_error;
use crate::operations;
use crate::pm::{self, with_project, OPFS_PROJECT};
use crate::tokio_runtime::init_tokio_runtime;

#[cfg(feature = "utoopack")]
use crate::pack::{self, RootTask};

static GLOBAL_THREAD_URL: RwLock<Option<String>> = RwLock::new(None);

#[wasm_bindgen]
pub struct Project;

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(js_name = init)]
    pub fn init(thread_url: String) {
        operations::reset();

        let mut url_guard = GLOBAL_THREAD_URL.write();
        if url_guard.is_none() && !thread_url.is_empty() {
            *url_guard = Some(thread_url.clone());
        }
        let final_url = url_guard.as_ref().cloned().unwrap_or(thread_url);
        drop(url_guard);

        if !final_url.is_empty() {
            init_tokio_runtime(final_url);
        }

        // Initialise the global OpfsProject if not already done
        let mut guard = OPFS_PROJECT.write();
        if guard.is_none() {
            *guard = Some(Arc::new(opfs_project::OpfsProject::default()));
        }
    }

    #[wasm_bindgen(js_name = setCwd)]
    pub fn set_cwd(path: String) {
        with_project(|p| p.set_cwd(path));
    }

    #[wasm_bindgen(getter)]
    pub fn cwd() -> String {
        with_project(|p| p.cwd().to_string_lossy().to_string())
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
        match operations::run_local(pm::install(&package_lock, max_concurrent_downloads)).await {
            Ok(result) => result.map_err(to_js_error),
            Err(_) => Err(JsError::new("AbortError: install was disposed")),
        }
    }

    #[cfg(feature = "utoopack")]
    #[wasm_bindgen]
    pub async fn build(options: pack::BuildOptions) -> Result<JsValue, JsError> {
        pack::build(options).await
    }

    #[cfg(not(feature = "utoopack"))]
    #[wasm_bindgen]
    pub async fn build(options: JsValue) -> Result<(), JsValue> {
        let _ = options;
        unimplemented!()
    }

    /// Cancel active install/build work and release project-owned state.
    #[wasm_bindgen]
    pub async fn dispose() -> Result<(), JsError> {
        operations::cancel_all().await;

        #[cfg(feature = "utoopack")]
        pack::dispose_pack_project().await;

        OPFS_PROJECT.write().take();
        Ok(())
    }

    /// Subscribe to entrypoints changes with HMR support.
    /// This will watch for file changes and automatically rebuild.
    /// Returns a RootTask that must be held by JS to keep the subscription active.
    #[cfg(feature = "utoopack")]
    #[wasm_bindgen(js_name = entrypointsSubscribe)]
    pub async fn entrypoints_subscribe(
        config: JsValue,
        callback: js_sys::Function,
    ) -> Result<RootTask, JsError> {
        let config_str = if config.is_undefined() || config.is_null() {
            None
        } else {
            let json_str = js_sys::JSON::stringify(&config)
                .map_err(|e| JsError::new(&format!("invalid config: {e:?}")))?;
            Some(json_str.as_string().unwrap_or_default())
        };
        pack::project_entrypoints_subscribe(config_str, callback).await
    }

    /// Subscribe to HMR events for a specific identifier.
    /// Returns a RootTask that must be held by JS to keep the subscription active.
    #[cfg(feature = "utoopack")]
    #[wasm_bindgen(js_name = hmrEvents)]
    pub async fn hmr_events(
        identifier: String,
        callback: js_sys::Function,
    ) -> Result<RootTask, JsError> {
        pack::project_hmr_events(identifier, callback).await
    }

    /// Subscribe to compilation lifecycle events.
    /// Emits "start" when computation begins, "end" when idle for aggregation_ms.
    #[cfg(feature = "utoopack")]
    #[wasm_bindgen(js_name = updateInfoSubscribe)]
    pub fn update_info_subscribe(aggregation_ms: u32, callback: js_sys::Function) {
        pack::project_update_info_subscribe(aggregation_ms, callback)
    }

    /// Write all entrypoints to disk.
    #[cfg(feature = "utoopack")]
    #[wasm_bindgen(js_name = writeAllToDisk)]
    pub fn write_all_to_disk(callback: js_sys::Function) {
        pack::project_write_all_to_disk(callback)
    }
}
