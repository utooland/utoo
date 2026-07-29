//! OPFS filesystem API and Glob implementation.
//!
//! Fuse-aware reads go through `OpfsProject`. All other ops use `tokio_fs_ext` directly.

use std::path::{Path, PathBuf};
use std::str;

use anyhow::Context;
use tokio_fs_ext::{DirEntry as RawDirEntry, Metadata as RawMetadata};
use utoo_ruborist::service::Glob;
use wasm_bindgen::prelude::*;

use crate::errors::to_js_error;
use crate::pm::OPFS_PROJECT;
use crate::tokio_runtime::runtime;

/// OPFS-backed glob implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpfsGlob;

impl Glob for OpfsGlob {
    type Error = anyhow::Error;

    async fn glob(&self, pattern: &Path) -> Result<Vec<PathBuf>, Self::Error> {
        glob_match(self, pattern).await
    }
}

/// Check if a path exists in OPFS.
async fn exists(path: &Path) -> Result<bool, anyhow::Error> {
    match tokio_fs_ext::metadata(path).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Simple glob pattern matching for OPFS.
///
/// Supports patterns like `packages/*/package.json` where `*` matches any single directory.
async fn glob_match(_glob: &OpfsGlob, pattern: &Path) -> Result<Vec<PathBuf>, anyhow::Error> {
    let pattern_str = pattern.to_string_lossy();
    let components: Vec<&str> = pattern_str.split('/').collect();

    let mut results = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];

    while let Some((current_path, idx)) = stack.pop() {
        if idx >= components.len() {
            // Reached end of pattern, check if path exists
            if exists(&current_path).await? {
                results.push(current_path);
            }
            continue;
        }

        let component = components[idx];

        if component == "*" {
            // Wildcard: list directory and match all entries
            let dir_path = if current_path.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                current_path.clone()
            };

            let project = OPFS_PROJECT.read().as_ref().cloned();
            let read_result = if let Some(project) = project {
                project.read_dir(&dir_path).await
            } else {
                tokio_fs_ext::read_dir(&dir_path)
                    .await
                    .and_then(|rd| rd.collect())
            };
            match read_result {
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(name) = entry.file_name().into_string() {
                            let next_path = current_path.join(&name);
                            stack.push((next_path, idx + 1));
                        }
                    }
                }
                Err(_) => {
                    // Directory doesn't exist, skip
                }
            }
        } else {
            // Literal component
            let next_path = current_path.join(component);
            stack.push((next_path, idx + 1));
        }
    }

    Ok(results)
}

// ============================================================================
// Filesystem API
// ============================================================================

#[wasm_bindgen]
pub struct Fs;

#[wasm_bindgen]
impl Fs {
    #[wasm_bindgen]
    pub async fn read(path: &str) -> Result<js_sys::Uint8Array, JsError> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| JsError::new("not initialised"))?;
        let bytes = project
            .read(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)?;
        Ok(js_sys::Uint8Array::from(&bytes[..]))
    }

    #[wasm_bindgen(js_name = readToString)]
    pub async fn read_to_string(path: &str) -> Result<String, JsError> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| JsError::new("not initialised"))?;
        let buf = project
            .read(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(to_js_error)?;
        Ok(unsafe { str::from_utf8_unchecked(&buf).to_owned() })
    }

    #[wasm_bindgen]
    pub async fn write(path: &str, content: js_sys::Uint8Array) -> Result<(), JsError> {
        let content = content.to_vec();
        tokio_fs_ext::write(path, &content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "writeString")]
    pub async fn write_string(path: &str, content: &str) -> Result<(), JsError> {
        tokio_fs_ext::write(path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(path: &str) -> Result<Vec<DirEntry>, JsError> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| JsError::new("not initialised"))?;
        let read_dir = project
            .read_dir(path)
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
        tokio_fs_ext::create_dir(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(path: &str) -> Result<(), JsError> {
        tokio_fs_ext::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory recursively: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(src: &str, dst: &str) -> Result<(), JsError> {
        tokio_fs_ext::copy(src, dst)
            .await
            .with_context(|| format!("Failed to copy file from {} to {}", src, dst))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeFile)]
    pub async fn remove_file(path: &str) -> Result<(), JsError> {
        tokio_fs_ext::remove_file(path)
            .await
            .with_context(|| format!("Failed to remove file: {}", path))
            .map_err(to_js_error)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = removeDir)]
    pub async fn remove_dir(path: &str, recursive: bool) -> Result<(), JsError> {
        if recursive {
            tokio_fs_ext::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to remove directory recursively: {}", path))
                .map_err(to_js_error)?;
        } else {
            tokio_fs_ext::remove_dir(path)
                .await
                .with_context(|| format!("Failed to remove directory: {}", path))
                .map_err(to_js_error)?;
        }

        Ok(())
    }

    #[wasm_bindgen(js_name = metadata)]
    pub async fn metadata(path: &str) -> Result<Metadata, JsError> {
        tokio_fs_ext::metadata(path)
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

        let bytes = runtime()
            .block_on(fut)
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

        let read_dir = runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
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

        runtime()
            .block_on(fut)
            .and_then(Metadata::try_from)
            .with_context(|| format!("Failed to get metadata: {}", path))
            .map_err(to_js_error)
    }
}

// ============================================================================
// Types
// ============================================================================

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
