//! File system utilities and Glob trait for pattern matching.
//!
//! This module provides:
//! - Helper functions wrapping tokio-fs-ext for common file operations
//! - `Glob` trait for pattern matching (separated for cleaner abstraction)
//!
//! # Design
//!
//! File operations use `tokio-fs-ext` directly, which handles cross-platform
//! differences (native vs WASM/OPFS) internally.
//!
//! The `Glob` trait is kept as an abstraction because:
//! - Native platforms can use the `glob` crate for full pattern support
//! - WASM can implement glob using read_dir with simple pattern matching

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};

/// Read a file and return its contents as a UTF-8 string.
pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    tokio_fs_ext::read_to_string(path).await
}

/// Check if a file or directory exists.
pub async fn exists(path: impl AsRef<Path>) -> bool {
    tokio_fs_ext::try_exists(path).await.unwrap_or(false)
}

/// Read a JSON file and deserialize it.
///
/// Uses `read` + `from_slice` instead of `read_to_string` + `from_str`
/// to avoid unnecessary UTF-8 string conversion.
pub async fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> io::Result<T> {
    let bytes = tokio_fs_ext::read(path).await?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Glob pattern matching trait.
///
/// Separated from file operations for cleaner abstraction:
/// - Native: Use `glob` crate for full pattern support (`**`, `{a,b}`, etc.)
/// - WASM: Can implement using read_dir with simple matching
#[cfg(not(target_arch = "wasm32"))]
pub trait Glob: Send + Sync {
    type Error: std::error::Error + Send + 'static;

    /// Match files using a glob pattern.
    ///
    /// Returns a list of paths that match the pattern.
    /// The pattern follows standard glob syntax (e.g., `packages/*/package.json`).
    fn glob(
        &self,
        pattern: &Path,
    ) -> impl Future<Output = Result<Vec<PathBuf>, Self::Error>> + Send;
}

/// WASM version without Send bounds.
#[cfg(target_arch = "wasm32")]
pub trait Glob {
    type Error: std::fmt::Debug + std::fmt::Display + 'static;

    /// Match files using a glob pattern.
    fn glob(&self, pattern: &Path) -> impl Future<Output = Result<Vec<PathBuf>, Self::Error>>;
}

/// No-op glob implementation for testing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopGlob;

impl Glob for NoopGlob {
    type Error = std::io::Error;

    async fn glob(&self, _pattern: &Path) -> Result<Vec<PathBuf>, Self::Error> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_glob() {
        let glob = NoopGlob;
        let pattern = PathBuf::from("packages/*/package.json");
        let result = glob.glob(&pattern).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_exists_nonexistent() {
        let result = exists("/nonexistent/path/that/should/not/exist").await;
        assert!(!result);
    }
}
