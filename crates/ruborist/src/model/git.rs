//! Git package resolution types.

use std::path::PathBuf;

use crate::model::manifest::CoreVersionManifest;

/// Metadata returned after cloning and caching a git package.
///
/// # Clone cost
///
/// Cloning this struct performs a full heap allocation for all fields.
/// In the clone-dedup cache this type is wrapped in [`Arc`](std::sync::Arc)
/// so concurrent waiters pay only a reference-count increment; avoid bare
/// `clone()` calls outside of that context.
#[derive(Debug, Clone)]
pub struct GitCloneResult {
    /// Package name from the cloned `package.json`.
    pub name: String,
    /// Package version from the cloned `package.json`.
    pub version: String,
    /// Local path to the cached package directory.
    pub path: PathBuf,
    /// Pinned URL, e.g. `git+https://github.com/user/repo.git#<sha>`.
    pub resolved_url: String,
    /// Slim manifest from the cloned `package.json`.
    ///
    /// Only the fields needed for resolution, installation, and lockfile
    /// serialization are kept. The `dist.tarball` field is set to
    /// [`resolved_url`](Self::resolved_url) so callers can use this
    /// directly as a [`ResolvedPackage`] manifest without any further I/O.
    ///
    /// [`ResolvedPackage`]: crate::traits::registry::ResolvedPackage
    pub manifest: CoreVersionManifest,
}

impl GitCloneResult {
    /// Construct a new `GitCloneResult`, asserting that the top-level fields
    /// are consistent with the embedded manifest.
    pub fn new(path: PathBuf, resolved_url: String, manifest: CoreVersionManifest) -> Self {
        let name = manifest.name.clone();
        let version = manifest.version.clone();
        debug_assert_eq!(
            manifest.dist.tarball.as_deref(),
            Some(resolved_url.as_str()),
            "manifest.dist.tarball must equal resolved_url"
        );
        Self {
            name,
            version,
            path,
            resolved_url,
            manifest,
        }
    }
}
