use std::path::PathBuf;
use std::str::FromStr;
#[cfg(feature = "utoopack")]
use std::sync::Arc;

use anyhow::Context;
use pack_api::project::WatchOptions;
use serde_wasm_bindgen::to_value;
use tokio_fs_ext::{DirEntry as RawDirEntry, Metadata as RawMetadata};
use turbo_rcstr::rcstr;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

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

fn block_on<T>(fut: impl std::future::Future<Output = T> + Send + 'static) -> Result<T, JsError>
where
    T: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let rt = crate::tokio_runtime::TOKIO_RUNTIME
        .get()
        .ok_or_else(|| JsError::new("tokio runtime not initialized"))?;

    rt.spawn(async move {
        let _ = sender.send(fut.await);
    });

    receiver
        .recv()
        .map_err(|e| JsError::new(&format!("Recv error: {}", e)))
}

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

    /// Lazy install - downloads tgz files only, extracts on-demand when files are read
    /// This is much faster than full extraction since:
    /// 1. Only downloads tgz files (no OPFS writes for individual files)
    /// 2. Files are extracted from tgz on first read
    #[wasm_bindgen(js_name = installParallel)]
    pub async fn install_parallel(package_lock: String) -> Result<(), JsError> {
        use opfs_project::{PublicPackagePaths, is_tgz_cached, download_only, create_fuse_links_lazy};
        use opfs_project::package_lock::PackageLock;
        use std::collections::HashMap;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let total_start = js_sys::Date::now();
        tracing::info!("[installLazy] Starting lazy install (no extraction)...");

        let lock = PackageLock::from_json(&package_lock)
            .map_err(|e| JsError::new(&format!("Failed to parse package-lock.json: {}", e)))?;

        // Write package.json to root
        if let Some(root_pkg) = lock.packages.get("") {
            if tokio_fs_ext::metadata("./package.json").await.is_err() {
                let pkg_json = serde_json::to_string_pretty(root_pkg).unwrap_or("{}".to_string());
                tokio_fs_ext::create_dir_all("./node_modules").await.map_err(to_js_error)?;
                tokio_fs_ext::write("./package.json", pkg_json.as_bytes()).await.map_err(to_js_error)?;
            }
        }

        // Internal package group structure
        struct PackageGroup {
            name: String,
            version: String,
            tgz_url: String,
            integrity: Option<String>,
            shasum: Option<String>,
            target_paths: Vec<String>,
        }

        // Step 1: Group packages by tgz URL to deduplicate downloads
        let mut groups: HashMap<String, PackageGroup> = HashMap::new();

        for (path, pkg) in lock.packages.iter().filter(|(path, _)| !path.is_empty()) {
            let name = pkg.get_name(path);
            let version = pkg.get_version();
            let tgz_url = match &pkg.resolved {
                Some(u) => u.clone(),
                None => {
                    return Err(JsError::new(&format!("{}@{}: no resolved field", name, version)));
                }
            };

            groups
                .entry(tgz_url.clone())
                .or_insert_with(|| PackageGroup {
                    name,
                    version,
                    tgz_url,
                    integrity: pkg.integrity.clone(),
                    shasum: pkg.shasum.clone(),
                    target_paths: Vec::new(),
                })
                .target_paths
                .push(path.clone());
        }

        let total_groups = groups.len();
        tracing::info!("[installLazy] Total unique packages: {}", total_groups);

        // Step 2: Partition by cache status (check if tgz already downloaded)
        let mut cached: Vec<(PathBuf, Vec<String>)> = Vec::new();
        let mut to_download: Vec<PackageGroup> = Vec::new();

        for group in groups.into_values() {
            let paths = PublicPackagePaths::new(&group.name, &group.tgz_url);
            if is_tgz_cached(&paths).await {
                cached.push((paths.tgz_store_path, group.target_paths));
            } else {
                to_download.push(group);
            }
        }

        let cached_count = cached.len();
        let download_count = to_download.len();
        tracing::info!("[installLazy] Cached: {}, To download: {}", cached_count, download_count);

        // Step 3: Create lazy fuse links for cached packages (pointing to tgz)
        if !cached.is_empty() {
            tracing::info!("[installLazy] Creating lazy fuse links for {} cached packages...", cached_count);
            for (tgz_path, target_paths) in cached {
                // npm tarballs have "package" prefix
                create_fuse_links_lazy(&tgz_path, &target_paths, Some("package"))
                    .await
                    .map_err(to_js_error)?;
            }
            tracing::info!("[installLazy] Cached packages linked");
        }

        // Download non-cached packages and create lazy fuse links
        if !to_download.is_empty() {
            let download_start = js_sys::Date::now();
            tracing::info!("[installLazy] Downloading {} packages...", download_count);
            let download_completed = Arc::new(AtomicUsize::new(0));

            let download_futures: Vec<_> = to_download
                .iter()
                .map(|group| {
                    let name = group.name.clone();
                    let version = group.version.clone();
                    let tgz_url = group.tgz_url.clone();
                    let integrity = group.integrity.clone();
                    let shasum = group.shasum.clone();
                    let completed = download_completed.clone();
                    let total = download_count;

                    async move {
                        tracing::debug!("[Download] Downloading {}@{}...", name, version);
                        // download_only downloads and saves tgz to OPFS
                        let _bytes = download_only(
                            &name,
                            &version,
                            &tgz_url,
                            integrity.as_deref(),
                            shasum.as_deref(),
                        )
                        .await?;
                        let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                        tracing::info!("[Download] Downloaded {}@{} ({}/{})", name, version, done, total);
                        Ok::<(String, String), anyhow::Error>((name, tgz_url))
                    }
                })
                .collect();

            let download_results = futures::future::join_all(download_futures).await;

            // Create lazy fuse links for downloaded packages
            for (result, group) in download_results.into_iter().zip(to_download.into_iter()) {
                let (name, tgz_url) = result.map_err(to_js_error)?;
                let paths = PublicPackagePaths::new(&name, &tgz_url);

                // Create lazy fuse links pointing to tgz (with "package" prefix)
                create_fuse_links_lazy(&paths.tgz_store_path, &group.target_paths, Some("package"))
                    .await
                    .map_err(to_js_error)?;
            }

            let download_elapsed = js_sys::Date::now() - download_start;
            tracing::info!(
                "[installLazy] Downloads complete: {} packages in {:.1}s",
                download_count,
                download_elapsed / 1000.0
            );
        }

        let total_elapsed = js_sys::Date::now() - total_start;
        tracing::info!("[installLazy] Install completed in {:.1}s", total_elapsed / 1000.0);

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

    #[wasm_bindgen]
    pub async fn read(path: &str) -> Result<js_sys::Uint8Array, JsError> {
        let bytes = opfs_project::read(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)?;
        Ok(js_sys::Uint8Array::from(&bytes[..]))
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
    pub async fn write(path: &str, content: js_sys::Uint8Array) -> Result<(), JsError> {
        let content = content.to_vec();
        opfs_project::write(path, &content)
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

    #[wasm_bindgen(js_name = readSync)]
    pub fn read_sync(path: String) -> Result<js_sys::Uint8Array, JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .read(&path_clone)
                .await
        };

        let bytes = block_on(fut)?
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)?;

        Ok(js_sys::Uint8Array::from(&bytes[..]))
    }

    #[wasm_bindgen(js_name = readDirSync)]
    pub fn read_dir_sync(path: String) -> Result<Vec<DirEntry>, JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .read_dir(&path_clone)
                .await
        };

        let read_dir = block_on(fut)?
            .with_context(|| format!("Failed to read directory: {}", path))
            .map_err(to_js_error)?;

        let ret = read_dir
            .map(|res| {
                let entry = res?;
                DirEntry::try_from(entry)
            })
            .collect::<Result<Vec<_>, std::io::Error>>()
            .with_context(|| format!("Failed to process directory entries: {}", path))
            .map_err(to_js_error)?;

        Ok(ret)
    }

    #[wasm_bindgen(js_name = writeSync)]
    pub fn write_sync(path: String, content: js_sys::Uint8Array) -> Result<(), JsError> {
        let content = content.to_vec();
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .write(&path_clone, &content)
                .await
        };

        block_on(fut)?
            .with_context(|| format!("Failed to write file: {}", path))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = createDirSync)]
    pub fn create_dir_sync(path: String) -> Result<(), JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .create_dir(&path_clone)
                .await
        };

        block_on(fut)?
            .with_context(|| format!("Failed to create directory: {}", path))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAllSync)]
    pub fn create_dir_all_sync(path: String) -> Result<(), JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .create_dir_all(&path_clone)
                .await
        };

        block_on(fut)?
            .with_context(|| format!("Failed to create directory recursively: {}", path))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = copyFileSync)]
    pub fn copy_file_sync(src: String, dst: String) -> Result<(), JsError> {
        let src_clone = src.clone();
        let dst_clone = dst.clone();
        let fut = async move {
            // Client doesn't seem to expose copy, so we implement it manually
            let content = turbo_tasks_fs::wasm_fs_offload::CLIENT
                .read(&src_clone)
                .await?;
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .write(&dst_clone, &content)
                .await
        };

        block_on(fut)?
            .with_context(|| format!("Failed to copy file from {} to {}", src, dst))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = removeFileSync)]
    pub fn remove_file_sync(path: String) -> Result<(), JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .remove_file(&path_clone)
                .await
        };

        block_on(fut)?
            .with_context(|| format!("Failed to remove file: {}", path))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = removeDirSync)]
    pub fn remove_dir_sync(path: String, recursive: bool) -> Result<(), JsError> {
        let path_clone = path.clone();
        let fut = async move {
            if recursive {
                turbo_tasks_fs::wasm_fs_offload::CLIENT
                    .remove_dir_all(&path_clone)
                    .await
            } else {
                turbo_tasks_fs::wasm_fs_offload::CLIENT
                    .remove_dir(&path_clone)
                    .await
            }
        };

        block_on(fut)?
            .with_context(|| format!("Failed to remove directory: {}", path))
            .map_err(to_js_error)?;

        Ok(())
    }

    #[wasm_bindgen(js_name = metadataSync)]
    pub fn metadata_sync(path: String) -> Result<Metadata, JsError> {
        let path_clone = path.clone();
        let fut = async move {
            turbo_tasks_fs::wasm_fs_offload::CLIENT
                .metadata(&path_clone)
                .await
        };

        block_on(fut)?
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
