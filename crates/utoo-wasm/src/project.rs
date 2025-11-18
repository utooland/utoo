use std::path::PathBuf;
use std::str::FromStr;
#[cfg(feature = "utoo-pack")]
use std::sync::Arc;

use anyhow::Context;
use pack_api::project::WatchOptions;
use serde_wasm_bindgen::to_value;
use tokio_fs_ext::{DirEntry as RawDirEntry, Metadata as RawMetadata};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

use crate::tokio_runtime::init_tokio_runtime;

#[cfg(feature = "utoo-pack")]
use super::{
    pack::{PackProject, PartialProjectOptions, TurbopackResult},
    tokio_runtime::TOKIO_RUNTIME,
};

use parking_lot::RwLock;

#[wasm_bindgen]
pub struct Project {
    #[cfg(feature = "utoo-pack")]
    pack_project: RwLock<Option<Arc<PackProject>>>,
}

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(constructor)]
    pub fn new(cwd: String, thread_url: String) -> Project {
        opfs_project::set_cwd(&cwd);
        init_tokio_runtime(thread_url);
        Project {
            #[cfg(feature = "utoo-pack")]
            pack_project: RwLock::new(None),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn cwd(&self) -> String {
        opfs_project::get_cwd().to_string_lossy().to_string()
    }

    /// Calculate MD5 hash of byte content
    #[wasm_bindgen(js_name = sigMd5)]
    pub fn sig_md5(&self, content: &[u8]) -> String {
        opfs_project::pack::sig_md5(content)
    }

    /// Create a tar.gz archive and return bytes (no file I/O)
    /// This is useful for main thread execution without OPFS access
    #[wasm_bindgen(js_name = gzip)]
    pub fn gzip(&self, files: JsValue) -> Result<js_sys::Uint8Array, String> {
        use opfs_project::pack::PackFile;
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct JsPackFile {
            path: String,
            content: Vec<u8>,
        }

        let js_files: Vec<JsPackFile> = serde_wasm_bindgen::from_value(files)
            .map_err(|e| format!("Failed to parse files: {}", e))?;

        let pack_files: Vec<PackFile> = js_files
            .into_iter()
            .map(|f| PackFile::new(f.path, f.content))
            .collect();

        let bytes = opfs_project::pack::gzip(&pack_files).map_err(|e| e.to_string())?;
        Ok(js_sys::Uint8Array::from(&bytes[..]))
    }

    #[wasm_bindgen]
    pub async fn install(
        &self,
        package_lock: String,
        max_concurrent_downloads: Option<usize>,
    ) -> Result<(), String> {
        const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 10;
        let max_concurrent = max_concurrent_downloads.unwrap_or(DEFAULT_MAX_CONCURRENT_DOWNLOADS);
        opfs_project::package_manager::install_deps(&package_lock, max_concurrent)
            .await
            // format anyhow backtrace for better error display in JS
            .map_err(|e| format!("{:#?}", e))?;
        Ok(())
    }

    #[cfg(feature = "utoo-pack")]
    #[wasm_bindgen]
    pub async fn build(&self) -> Result<JsValue, String> {
        use turbopack_core::error::PrettyPrintError;

        self.init_pack_project()
            .await
            .map_err(|e| PrettyPrintError(&e).to_string())?;

        let pack_project = match self.pack_project.read().as_ref() {
            Some(pack_project) => pack_project.clone(),
            None => return Err("invalid pack project".to_string()),
        };

        TOKIO_RUNTIME
            .with(|rt| {
                rt.get()
                    .expect("tokio runtime not found")
                    .spawn(async move { pack_project.build().await })
            })
            .await
            .map_err(|e| e.to_string())?
            .map_or_else(
                |e| Err(PrettyPrintError(&e).to_string()),
                |turbopack_result| {
                    use serde::Serialize;

                    (&turbopack_result)
                        .serialize(
                            &serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true),
                        )
                        .map_err(|e| e.to_string())
                },
            )
    }

    async fn init_pack_project(&self) -> anyhow::Result<()> {
        if self.pack_project.read().is_none() {
            use pack_api::project::ProjectOptions;
            use turbo_rcstr::RcStr;

            let config = self.read_to_string("utoopack.json").await.ok();

            let partial_options = PartialProjectOptions {
                project_path: ".".into(),
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

            let mut pack_project_guard = self.pack_project.write();
            *pack_project_guard = Some(Arc::new(pack_context));
        }

        Ok(())
    }

    #[cfg(not(feature = "utoo-pack"))]
    #[wasm_bindgen]
    pub async fn build(&self) -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Build functionality requires the 'utoo-pack' feature to be enabled",
        ))
    }

    #[wasm_bindgen]
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        opfs_project::read(path).await.map_err(|e| e.to_string())
    }

    #[wasm_bindgen(js_name = readToString)]
    pub async fn read_to_string(&self, path: &str) -> Result<String, String> {
        let buf = opfs_project::read(path).await.map_err(|e| e.to_string())?;
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }

    #[wasm_bindgen]
    pub async fn write(&self, path: &str, content: &[u8]) -> Result<(), String> {
        opfs_project::write(path, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "writeString")]
    pub async fn write_string(&self, path: &str, content: &str) -> Result<(), String> {
        opfs_project::write(path, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, String> {
        let read_dir = opfs_project::read_dir(path)
            .await
            .map_err(|e| e.to_string())?;

        let ret = read_dir
            .into_iter()
            .map(DirEntry::try_from)
            .collect::<Result<Vec<_>, std::io::Error>>()
            .map_err(|e| e.to_string())?;

        Ok(ret)
    }

    #[wasm_bindgen(js_name = createDir)]
    pub async fn create_dir(&self, path: &str) -> Result<(), String> {
        opfs_project::create_dir(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(&self, path: &str) -> Result<(), String> {
        opfs_project::create_dir_all(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        opfs_project::copy(src, dst)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeFile)]
    pub async fn remove_file(&self, path: &str) -> Result<(), String> {
        opfs_project::remove_file(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeDir)]
    pub async fn remove_dir(&self, path: &str, recursive: bool) -> Result<(), String> {
        if recursive {
            opfs_project::remove_dir_all(path)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            opfs_project::remove_dir(path)
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    #[wasm_bindgen(js_name = metadata)]
    pub async fn metadata(&self, path: &str) -> Result<Metadata, String> {
        opfs_project::metadata(path)
            .await
            .and_then(Metadata::try_from)
            .map_err(|e| e.to_string())
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
    r#type: DirEntryType,
    file_size: u64,
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
