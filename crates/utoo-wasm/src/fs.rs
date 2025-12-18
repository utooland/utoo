//! OPFS FileSystem implementation for WASM environment.
//!
//! Implements ruborist's `FileSystem` trait using `opfs_project` bindings.

use std::path::{Path, PathBuf};
use utoo_ruborist::service::{FileSystem, Glob};

/// OPFS-backed file system for WASM environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpfsFileSystem;

impl FileSystem for OpfsFileSystem {
    type Error = anyhow::Error;

    async fn read(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: {}", path.display()))?;
        opfs_project::read(path_str)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, Self::Error> {
        let bytes = self.read(path).await?;
        String::from_utf8(bytes)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in {}: {}", path.display(), e))
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<(), Self::Error> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: {}", path.display()))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.create_dir_all(parent).await?;
            }
        }

        opfs_project::write(path_str, content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", path.display(), e))
    }

    async fn exists(&self, path: &Path) -> Result<bool, Self::Error> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: {}", path.display()))?;

        // Try to get metadata - if it fails, the path doesn't exist
        match opfs_project::metadata(path_str).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn create_dir_all(&self, path: &Path) -> Result<(), Self::Error> {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path: {}", path.display()))?;

        opfs_project::create_dir_all(path_str)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", path.display(), e))
    }
}

impl Glob for OpfsFileSystem {
    type Error = anyhow::Error;

    async fn glob(&self, pattern: &Path) -> Result<Vec<PathBuf>, Self::Error> {
        glob_match(self, pattern).await
    }
}

/// Simple glob pattern matching for OPFS.
///
/// Supports patterns like `packages/*/package.json` where `*` matches any single directory.
async fn glob_match(fs: &OpfsFileSystem, pattern: &Path) -> Result<Vec<PathBuf>, anyhow::Error> {
    let pattern_str = pattern.to_string_lossy();
    let components: Vec<&str> = pattern_str.split('/').collect();

    let mut results = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(PathBuf::new(), 0)];

    while let Some((current_path, idx)) = stack.pop() {
        if idx >= components.len() {
            // Reached end of pattern, check if path exists
            if fs.exists(&current_path).await? {
                results.push(current_path);
            }
            continue;
        }

        let component = components[idx];

        if component == "*" {
            // Wildcard: list directory and match all entries
            let dir_path = if current_path.as_os_str().is_empty() {
                ".".to_string()
            } else {
                current_path.to_string_lossy().to_string()
            };

            match opfs_project::read_dir(&dir_path).await {
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
