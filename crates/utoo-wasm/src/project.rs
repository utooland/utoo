use std::path::PathBuf;
use std::str::FromStr;
#[cfg(feature = "utoopack")]
use std::sync::Arc;

use anyhow::Context;
use pack_api::project::WatchOptions;
use serde_wasm_bindgen::to_value;
use tokio_fs_ext::{DirEntry as RawDirEntry, Metadata as RawMetadata};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

use crate::errors::to_js_error;
use crate::tokio_runtime::init_tokio_runtime;

#[cfg(feature = "utoopack")]
use super::{
    pack::{PackProject, PartialProjectOptions, TurbopackResult},
    tokio_runtime::TOKIO_RUNTIME,
};

use parking_lot::RwLock;
use std::sync::Once;

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
    pub async fn sig_md5(content: Vec<u8>) -> Result<String, JsError> {
        let result = tokio::task::spawn_blocking(move || opfs_project::pack::sig_md5(&content))
            .await
            .map_err(|e| JsError::new(&format!("Task failed: {}", e)))?;
        Ok(result)
    }

    /// Create a tar.gz archive and return bytes (no file I/O)
    /// This is useful for main thread execution without OPFS access
    #[wasm_bindgen(js_name = gzip)]
    pub async fn gzip(files: JsValue) -> Result<js_sys::Uint8Array, JsError> {
        use opfs_project::pack::PackFile;
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct JsPackFile {
            path: String,
            content: Vec<u8>,
        }

        let js_files: Vec<JsPackFile> = serde_wasm_bindgen::from_value(files)
            .map_err(|e| JsError::new(&format!("Failed to parse files: {}", e)))?;

        let pack_files: Vec<PackFile> = js_files
            .into_iter()
            .map(|f| PackFile::new(f.path, f.content))
            .collect();

        let bytes = tokio::task::spawn_blocking(move || opfs_project::pack::gzip(&pack_files))
            .await
            .map_err(|e| JsError::new(&format!("Task failed: {}", e)))?
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
        &self,
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

    #[wasm_bindgen]
    pub async fn install(
        package_lock: String,
        max_concurrent_downloads: Option<usize>,
    ) -> Result<(), JsError> {
        const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 20;
        let max_concurrent = max_concurrent_downloads.unwrap_or(DEFAULT_MAX_CONCURRENT_DOWNLOADS);
        opfs_project::package_manager::install_deps(&package_lock, max_concurrent)
            .await
            // format anyhow backtrace for better error display in JS
            .map_err(to_js_error)?;
        Ok(())
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

        TOKIO_RUNTIME
            .with(|rt| {
                rt.get()
                    .expect("tokio runtime not found")
                    .spawn(async move { pack_project.build().await })
            })
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
                format!("/{}", cwd)
            };

            let config_path = std::path::PathBuf::from(&project_root)
                .join("utoopack.json")
                .to_string_lossy()
                .to_string();

            let config = Self::read_to_string(&config_path).await.ok();

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
                ..Default::default()
            };

            let pack_context = TOKIO_RUNTIME
                .with(|rt| {
                    rt.get()
                        .expect("tokio runtime not found")
                        .spawn(PackProject::initialize(options))
                })
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

    #[wasm_bindgen]
    pub async fn read(path: &str) -> Result<Vec<u8>, JsError> {
        opfs_project::read(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)
    }

    #[wasm_bindgen(js_name = readToString)]
    pub async fn read_to_string(path: &str) -> Result<String, JsError> {
        let buf = opfs_project::read(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)?;
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }

    #[wasm_bindgen]
    pub async fn write(path: &str, content: &[u8]) -> Result<(), JsError> {
        opfs_project::write(path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "writeString")]
    pub async fn write_string(path: &str, content: &str) -> Result<(), JsError> {
        opfs_project::write(path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(path: &str) -> Result<Vec<DirEntry>, JsError> {
        let read_dir = opfs_project::read_dir(path)
            .await
            .with_context(|| format!("Failed to read directory: {}", path))
            .map_err(to_js_error)?;

        let ret = read_dir
            .into_iter()
            .map(DirEntry::try_from)
            .collect::<Result<Vec<_>, std::io::Error>>()
            .with_context(|| format!("Failed to process directory entries: {}", path))
            .map_err(to_js_error)?;

        Ok(ret)
    }

    #[wasm_bindgen(js_name = createDir)]
    pub async fn create_dir(path: &str) -> Result<(), JsError> {
        opfs_project::create_dir(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(path: &str) -> Result<(), JsError> {
        opfs_project::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory recursively: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(src: &str, dst: &str) -> Result<(), JsError> {
        opfs_project::copy(src, dst)
            .await
            .with_context(|| format!("Failed to copy file from {} to {}", src, dst))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeFile)]
    pub async fn remove_file(path: &str) -> Result<(), JsError> {
        opfs_project::remove_file(path)
            .await
            .with_context(|| format!("Failed to remove file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeDir)]
    pub async fn remove_dir(path: &str, recursive: bool) -> Result<(), JsError> {
        if recursive {
            opfs_project::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to remove directory recursively: {}", path))
                .map_err(to_js_error)?;
        } else {
            opfs_project::remove_dir(path)
                .await
                .with_context(|| format!("Failed to remove directory: {}", path))
                .map_err(to_js_error)?;
        }

        Ok(())
    }

    #[wasm_bindgen(js_name = metadata)]
    pub async fn metadata(path: &str) -> Result<Metadata, JsError> {
        opfs_project::metadata(path)
            .await
            .and_then(Metadata::try_from)
            .with_context(|| format!("Failed to get metadata: {}", path))
            .map_err(to_js_error)
    }
}

#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    #[wasm_bindgen(getter_with_clone)]
    pub name: String,
    #[wasm_bindgen]
    pub r#type: DirEntryType,
}

#[wasm_bindgen]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DirEntryType {
    File = "file",
    Directory = "directory",
}

impl TryFrom<RawDirEntry> for DirEntry {
    type Error = std::io::Error;

    fn try_from(raw: RawDirEntry) -> Result<Self, Self::Error> {
        Ok(DirEntry {
            r#type: {
                let file_type = raw.file_type()?;
                if file_type.is_dir() {
                    DirEntryType::Directory
                } else if file_type.is_file() {
                    DirEntryType::File
                } else {
                    return Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
                }
            },
            name: raw.file_name().to_string_lossy().to_string(),
        })
    }
}

#[wasm_bindgen(inspectable)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Metadata {
    #[wasm_bindgen]
    pub r#type: DirEntryType,
    pub file_size: u64,
}

impl TryFrom<RawMetadata> for Metadata {
    type Error = std::io::Error;

    fn try_from(raw: RawMetadata) -> Result<Self, Self::Error> {
        Ok(Metadata {
            r#type: if raw.is_file() {
                DirEntryType::File
            } else if raw.is_dir() {
                DirEntryType::Directory
            } else {
                return Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
            },
            file_size: raw.len(),
        })
    }
}
