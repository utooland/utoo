//! OPFS Glob implementation for WASM environment.
//!
//! Implements ruborist's `Glob` trait using `opfs_project` bindings.

use std::path::{Path, PathBuf};
use utoo_ruborist::service::Glob;

/// OPFS-backed glob for WASM environment.
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
    let path_str = path.to_string_lossy();

    // Try to get metadata - if it fails, the path doesn't exist
    match opfs_project::metadata(&path_str).await {
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
