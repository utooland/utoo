//! FileSystem trait for platform-agnostic file I/O.
//!
//! Minimal async file system abstraction for cross-platform use.
//! Implementations are provided by users (e.g., TokioFileSystem in pm, OPFS in WASM).

use std::future::Future;
use std::path::{Path, PathBuf};

/// Minimal async file system trait.
///
/// # Platform bounds
/// - Native: `Send + Sync` for async runtime compatibility
/// - WASM: No `Send` (single-threaded)
#[cfg(not(target_arch = "wasm32"))]
pub trait FileSystem: Send + Sync {
    type Error: std::error::Error + Send + 'static;

    fn read(&self, path: &Path) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
    fn read_to_string(
        &self,
        path: &Path,
    ) -> impl Future<Output = Result<String, Self::Error>> + Send;
    fn write(
        &self,
        path: &Path,
        content: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn exists(&self, path: &Path) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    fn create_dir_all(&self, path: &Path) -> impl Future<Output = Result<(), Self::Error>> + Send;

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
pub trait FileSystem {
    type Error: std::fmt::Debug + std::fmt::Display + 'static;

    fn read(&self, path: &Path) -> impl Future<Output = Result<Vec<u8>, Self::Error>>;
    fn read_to_string(&self, path: &Path) -> impl Future<Output = Result<String, Self::Error>>;
    fn write(&self, path: &Path, content: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    fn exists(&self, path: &Path) -> impl Future<Output = Result<bool, Self::Error>>;
    fn create_dir_all(&self, path: &Path) -> impl Future<Output = Result<(), Self::Error>>;

    /// Match files using a glob pattern.
    ///
    /// Returns a list of paths that match the pattern.
    /// The pattern follows standard glob syntax (e.g., `packages/*/package.json`).
    fn glob(&self, pattern: &Path) -> impl Future<Output = Result<Vec<PathBuf>, Self::Error>>;
}

/// No-op file system for environments without file I/O.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopFileSystem;

impl FileSystem for NoopFileSystem {
    type Error = std::io::Error;

    async fn read(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("NoopFileSystem: {}", path.display()),
        ))
    }

    async fn read_to_string(&self, path: &Path) -> Result<String, Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("NoopFileSystem: {}", path.display()),
        ))
    }

    async fn write(&self, path: &Path, _content: &[u8]) -> Result<(), Self::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("NoopFileSystem: {}", path.display()),
        ))
    }

    async fn exists(&self, _path: &Path) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn create_dir_all(&self, _path: &Path) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn glob(&self, _pattern: &Path) -> Result<Vec<PathBuf>, Self::Error> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_noop_filesystem() {
        let fs = NoopFileSystem;
        let path = PathBuf::from("/test/path");

        assert!(fs.read(&path).await.is_err());
        assert!(fs.read_to_string(&path).await.is_err());
        assert!(fs.write(&path, b"test").await.is_err());
        assert!(!fs.exists(&path).await.unwrap());
        assert!(fs.create_dir_all(&path).await.is_ok());
    }
}
