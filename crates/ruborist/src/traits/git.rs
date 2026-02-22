//! Git package resolution.
//!
//! Spec parsing, manifest building, and `ResolvedPackage` construction all
//! live here.  The actual `git clone` is provided by [`crate::resolver::git`]
//! when the `git` Cargo feature is enabled.

#[cfg(feature = "native-git")]
use crate::model::manifest::{Dist, VersionManifest};
#[cfg(feature = "native-git")]
use crate::model::spec::{PackageSpec, parse_cli_spec};
use crate::traits::registry::ResolvedPackage;
#[cfg(feature = "native-git")]
use std::collections::HashMap;
#[cfg(feature = "native-git")]
use std::path::Path;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Clone result
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// High-level resolver — called by BFS `process_dependency`
// ---------------------------------------------------------------------------

/// Resolve a non-registry dependency spec to a [`ResolvedPackage`].
///
/// 1. Parses the spec (`git+https://…`, `github:user/repo#ref`, …).
/// 2. Clones the repository (requires the `git` feature).
/// 3. Reads `package.json` from the cache and builds a [`VersionManifest`].
///
/// When the `git` feature is **disabled**, this always returns an error.
#[cfg(feature = "native-git")]
pub(crate) async fn resolve_non_registry_dep(
    cache_dir: &Option<PathBuf>,
    dep_name: &str,
    spec: &str,
) -> anyhow::Result<ResolvedPackage> {
    let parsed = parse_cli_spec(spec);

    let (url, commit_ish) = match &parsed {
        PackageSpec::Git { url, commit_ish } => (url.clone(), commit_ish.clone()),
        PackageSpec::GitHub {
            owner,
            repo,
            commit_ish,
        } => (
            format!("git+https://github.com/{owner}/{repo}.git"),
            commit_ish.clone(),
        ),
        _ => anyhow::bail!("Unsupported non-registry spec: {spec}"),
    };

    let cache_dir = cache_dir
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("cache_dir required for git dependency resolution"))?;

    let result = crate::resolver::git::clone_repo(cache_dir, &url, commit_ish.as_deref()).await?;

    let manifest = build_manifest_from_git_cache(
        &result.name,
        &result.version,
        &result.cache_path,
        &result.resolved_url,
    )?;

    Ok(ResolvedPackage {
        name: dep_name.to_string(),
        version: result.version,
        manifest,
    })
}

#[cfg(not(feature = "native-git"))]
pub(crate) async fn resolve_non_registry_dep(
    _cache_dir: &Option<PathBuf>,
    _dep_name: &str,
    spec: &str,
) -> anyhow::Result<ResolvedPackage> {
    anyhow::bail!(
        "Git resolution not available for spec '{spec}' (enable the 'native-git' feature)"
    )
}

// ---------------------------------------------------------------------------
// Manifest builder (private helpers)
// ---------------------------------------------------------------------------

/// Build a [`VersionManifest`] from a cached git package's `package.json`.
#[cfg(feature = "native-git")]
fn build_manifest_from_git_cache(
    name: &str,
    version: &str,
    cache_path: &Path,
    resolved_url: &str,
) -> anyhow::Result<VersionManifest> {
    let pkg_json_path = cache_path.join("package.json");
    let pkg_json_content = std::fs::read_to_string(&pkg_json_path)
        .map_err(|e| anyhow::anyhow!("Failed to read cached package.json for git package: {e}"))?;

    let pkg_json: serde_json::Value = serde_json::from_str(&pkg_json_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse cached package.json: {e}"))?;

    let dependencies = extract_dep_map(&pkg_json, "dependencies");
    let dev_dependencies = extract_dep_map(&pkg_json, "devDependencies");
    let peer_dependencies = extract_dep_map(&pkg_json, "peerDependencies");
    let optional_dependencies = extract_dep_map(&pkg_json, "optionalDependencies");
    let bin = pkg_json.get("bin").cloned();
    let scripts = extract_dep_map(&pkg_json, "scripts");
    let engines = extract_dep_map(&pkg_json, "engines");

    let has_install_script = scripts.as_ref().is_some_and(|s| {
        s.contains_key("preinstall") || s.contains_key("install") || s.contains_key("postinstall")
    });

    Ok(VersionManifest {
        name: name.to_string(),
        version: version.to_string(),
        dist: Dist {
            tarball: Some(resolved_url.to_string()),
            integrity: None,
            ..Default::default()
        },
        dependencies,
        dev_dependencies,
        peer_dependencies,
        optional_dependencies,
        bin,
        scripts,
        engines,
        has_install_script: Some(has_install_script),
        ..Default::default()
    })
}

#[cfg(feature = "native-git")]
fn extract_dep_map(pkg_json: &serde_json::Value, field: &str) -> Option<HashMap<String, String>> {
    pkg_json.get(field).and_then(|v| {
        v.as_object().map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
    })
}
