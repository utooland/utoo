use std::path::PathBuf;
#[cfg(feature = "utoopack")]
use std::sync::Arc;

use anyhow::Context;
use pack_api::project::WatchOptions;
use turbo_rcstr::rcstr;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

use crate::errors::to_js_error;
use crate::fs::Fs;
use crate::tokio_runtime::init_tokio_runtime;

#[cfg(feature = "utoopack")]
use super::{
    pack::{PackProject, PartialProjectOptions, TurbopackResult},
    tokio_runtime::TOKIO_RUNTIME,
};

use parking_lot::RwLock;

#[cfg(feature = "utoopack")]
static GLOBAL_PACK_PROJECT: RwLock<Option<Arc<PackProject>>> = RwLock::new(None);
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
        let content = content.to_vec();
        let rt = TOKIO_RUNTIME
            .get()
            .ok_or_else(|| JsError::new("tokio runtime not initialized"))?;

        let result = rt
            .spawn_blocking(move || opfs_project::pack::sig_md5(&content))
            .await
            .map_err(to_js_error)?;
        Ok(result)
    }

    /// Create a tar.gz archive and return bytes (no file I/O)
    /// This is useful for main thread execution without OPFS access
    #[wasm_bindgen(js_name = gzip)]
    pub async fn gzip(files: JsValue) -> Result<js_sys::Uint8Array, JsError> {
        use anyhow::anyhow;
        use opfs_project::pack::PackFile;

        let files_array: js_sys::Array = files
            .dyn_into()
            .map_err(|e| to_js_error(anyhow!("files must be an array: {:?}", e)))?;

        let mut pack_files: Vec<PackFile> = Vec::with_capacity(files_array.length() as usize);

        for i in 0..files_array.length() {
            let item = files_array.get(i);
            let path = js_sys::Reflect::get(&item, &"path".into())
                .map_err(|e| to_js_error(anyhow!("missing path at index {}: {:?}", i, e)))?
                .as_string()
                .ok_or_else(|| to_js_error(anyhow!("path must be a string at index {}", i)))?;
            let content_js = js_sys::Reflect::get(&item, &"content".into())
                .map_err(|e| to_js_error(anyhow!("missing content at index {}: {:?}", i, e)))?;
            let content_arr: js_sys::Uint8Array = content_js.dyn_into().map_err(|e| {
                to_js_error(anyhow!(
                    "content must be Uint8Array at index {}: {:?}",
                    i,
                    e
                ))
            })?;
            pack_files.push(PackFile::new(path, content_arr.to_vec()));
        }

        let rt = TOKIO_RUNTIME
            .get()
            .ok_or_else(|| to_js_error(anyhow!("tokio runtime not initialized")))?;

        let bytes = rt
            .spawn_blocking(move || opfs_project::pack::gzip(&pack_files))
            .await?
            .map_err(to_js_error)?;

        Ok(js_sys::Uint8Array::from(&bytes[..]))
    }

    /// Generate package-lock.json by resolving dependencies.
    ///
    /// # Arguments
    /// * `registry` - Optional registry URL. If None, uses npmmirror.
    ///   - "https://registry.npmmirror.com" - supports semver queries (faster)
    ///   - "https://registry.npmjs.org" - official npm registry (slower, fetches full manifest)
    /// * `concurrency` - Optional concurrency limit (defaults to 20)
    #[wasm_bindgen]
    pub async fn deps(
        registry: Option<String>,
        concurrency: Option<usize>,
    ) -> Result<String, String> {
        use std::path::Path;

        let cwd = opfs_project::get_cwd();
        let package_lock =
            crate::deps::build_deps_from_file(Path::new(&cwd), registry.as_deref(), concurrency)
                .await
                .map_err(|e| format!("{:#?}", e))?;

        // Serialize to JSON string
        serde_json::to_string_pretty(&package_lock)
            .map_err(|e| format!("Failed to serialize package lock: {}", e))
    }

    /// Install dependencies - downloads tgz files only, extracts on-demand when files are read
    #[wasm_bindgen]
    pub async fn install(
        package_lock: String,
        max_concurrent_downloads: Option<usize>,
    ) -> Result<(), JsError> {
        let concurrency = max_concurrent_downloads.unwrap_or(20);
        opfs_project::package_manager::install_deps(&package_lock, concurrency)
            .await
            .map_err(to_js_error)
    }

    #[cfg(feature = "utoopack")]
    #[wasm_bindgen]
    pub async fn build() -> Result<JsValue, JsError> {
        use turbopack_core::error::PrettyPrintError;

        Self::init_pack_project().await.map_err(to_js_error)?;

        let pack_project = match GLOBAL_PACK_PROJECT.read().as_ref() {
            Some(pack_project) => pack_project.clone(),
            None => return Err(JsError::new("invalid pack project")),
        };

        let rt = TOKIO_RUNTIME
            .get()
            .ok_or_else(|| JsError::new("tokio runtime not initialized"))?;

        rt.spawn(async move { pack_project.build().await })
            .await
            .map_err(to_js_error)?
            .map_or_else(
                |e| Err(JsError::new(&PrettyPrintError(&e).to_string())),
                |turbopack_result| {
                    use serde::Serialize;

                    (&turbopack_result)
                        .serialize(
                            &serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true),
                        )
                        .map_err(|e| JsError::new(&e.to_string()))
                },
            )
    }

    async fn init_pack_project() -> anyhow::Result<()> {
        if GLOBAL_PACK_PROJECT.read().is_none() {
            use pack_api::project::ProjectOptions;
            use turbo_rcstr::RcStr;

            let cwd = opfs_project::get_cwd().to_string_lossy().to_string();
            let project_root = if cwd.starts_with('/') {
                cwd
            } else {
                tokio_fs_ext::current_dir()?
                    .join(cwd)
                    .to_string_lossy()
                    .to_string()
            };

            let config_path = std::path::PathBuf::from(&project_root)
                .join("utoopack.json")
                .to_string_lossy()
                .to_string();

            let config = Fs::read_to_string(&config_path).await.ok();

            let partial_options = PartialProjectOptions {
                project_path: project_root,
                config,
            };
            let project_path: RcStr = partial_options.project_path.into();

            let config = partial_options.config.unwrap_or("{}".to_string()).into();
            let options = ProjectOptions {
                root_path: project_path.clone(),
                project_path: project_path.clone(),
                config,
                build_id: project_path.clone(),
                watch: WatchOptions {
                    enable: true,
                    ..Default::default()
                },
                define_env: Default::default(),
                dev: false,
                pack_path: rcstr!("./"),
                process_env: Default::default(),
            };

            let rt = TOKIO_RUNTIME
                .get()
                .ok_or_else(|| anyhow::anyhow!("tokio runtime not initialized"))?;
            let pack_context = rt
                .spawn(PackProject::initialize(options))
                .await
                .context("fail to initialize pack project")??;

            let mut pack_project_guard = GLOBAL_PACK_PROJECT.write();
            *pack_project_guard = Some(Arc::new(pack_context));
        }

        Ok(())
    }

    #[cfg(not(feature = "utoopack"))]
    #[wasm_bindgen]
    pub async fn build() -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Build functionality requires the 'utoopack' feature to be enabled",
        ))
    }
}
