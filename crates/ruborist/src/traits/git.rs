//! Git package resolution types.

use std::path::PathBuf;

/// Metadata returned by a git clone operation.
#[derive(Debug, Clone)]
pub struct GitCloneResult {
    /// Package name from the cloned `package.json`.
    pub name: String,
    /// Package version from the cloned `package.json`.
    pub version: String,
    /// Local path to the cached package directory (contains `package.json`).
    pub cache_path: PathBuf,
    /// Pinned URL, e.g. `git+https://github.com/user/repo.git#<sha>`.
    pub resolved_url: String,
}
