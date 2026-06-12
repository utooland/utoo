//! npm registry manifest types.
//!
//! These types represent the JSON responses from npm registry API.
//! Used by both PM (native) and WASM (browser) implementations.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::package_json::PackageJson;

/// Borrowed view of the data needed to resolve a version spec — a slice of
/// available versions plus a dist-tag map.
///
/// The version-resolution logic ([`crate::resolver::version::resolve_target_version`])
/// only needs read access to these two pieces of data; everything else on a
/// `FullManifest` (raw bytes, time map, maintainers, …) is irrelevant. By
/// borrowing them through a unified view we can serve the same resolver from
/// multiple in-memory shapes (a freshly-fetched `FullManifest`, a 304-cached
/// `VersionsInfo`, a disk-loaded `Versions`) without cloning data or
/// duplicating the resolution code.
///
/// The lifetime parameter ties the view to whatever the caller is holding,
/// so the borrow checker statically rejects any attempt to keep the view
/// alive past its source. In practice every call site uses the view inside
/// a single function body — the lifetime never escapes.
///
/// Construct via the `From` impls (defined alongside each source type):
/// ```ignore
/// // From a freshly-fetched manifest:
/// let view = VersionsRef::from(&*full_manifest);
/// // From the 304-path versions cache:
/// let view = VersionsRef::from(&*versions_info);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct VersionsRef<'a> {
    pub versions: &'a [String],
    pub dist_tags: &'a HashMap<String, String>,
}

impl<'a> From<&'a FullManifest> for VersionsRef<'a> {
    fn from(m: &'a FullManifest) -> Self {
        Self {
            versions: &m.versions,
            dist_tags: &m.dist_tags,
        }
    }
}

/// Skip on error - try to deserialize, return None if fails.
/// This handles malformed npm registry data gracefully.
fn skip_on_error<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
}

/// Full package manifest from npm registry.
/// This is the response from `GET /<package-name>` endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FullManifest {
    #[serde(rename = "_id")]
    pub id: Option<String>,

    #[serde(rename = "_rev")]
    pub rev: Option<String>,

    pub name: String,

    pub description: Option<String>,

    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,

    #[serde(default, deserialize_with = "deserialize_version_keys")]
    pub versions: Vec<String>,

    /// Raw HTTP response bytes. Injected post-parse; used for on-demand
    /// version extraction via [`extract_version`](Self::extract_version).
    /// Storing the raw bytes (rather than a parsed tree) keeps the cached
    /// `FullManifest` compact — npm `versions` subtrees parsed to a typed
    /// tree expand to ~1.5–2.5x the raw size on real-world packages.
    #[serde(skip)]
    pub raw: Bytes,

    /// Per-version raw JSON text, lazily split from [`raw`](Self::raw) on the
    /// first extraction and memoized. Splitting once turns N on-demand version
    /// extractions from N full-document re-parses into a single structural pass
    /// plus N cheap single-object parses — see [`extract_version`](Self::extract_version).
    /// An internal cache, like [`raw`](Self::raw); construct via `..Default::default()`.
    #[serde(skip)]
    pub version_blobs: OnceLock<HashMap<String, Box<str>>>,

    /// Parsed [`versions`](Self::versions), sorted descending, lazily built on
    /// the first client-side semver match that the `latest` dist-tag doesn't
    /// short-circuit. Every additional distinct spec for the package then walks
    /// the sorted list top-down and early-exits, instead of re-parsing the full
    /// version list per spec (react-scale packages: ~2k parses per spec, on the
    /// single-threaded resolver driver).
    #[serde(skip)]
    pub parsed_versions: OnceLock<Vec<deno_semver::Version>>,

    pub time: HashMap<String, String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub maintainers: Option<Vec<Maintainer>>,

    #[serde(deserialize_with = "skip_on_error")]
    pub author: Option<Author>,

    #[serde(deserialize_with = "skip_on_error")]
    pub repository: Option<Repository>,

    #[serde(deserialize_with = "skip_on_error")]
    pub bugs: Option<Bugs>,

    #[serde(deserialize_with = "skip_on_error")]
    pub homepage: Option<String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub keywords: Option<Vec<String>>,

    #[serde(deserialize_with = "skip_on_error")]
    pub license: Option<String>,

    #[serde(deserialize_with = "skip_on_error")]
    pub readme: Option<String>,

    #[serde(rename = "readmeFilename")]
    #[serde(deserialize_with = "skip_on_error")]
    pub readme_filename: Option<String>,
}

impl FullManifest {
    /// Lazily split [`raw`](Self::raw) into per-version JSON text, keyed by
    /// version. The split is a single shallow structural pass — version objects
    /// are captured as raw byte slices, not parsed — and is memoized so repeated
    /// extractions reuse it instead of re-parsing the whole document each time.
    fn version_blobs(&self) -> &HashMap<String, Box<str>> {
        self.version_blobs.get_or_init(|| {
            #[derive(Deserialize)]
            struct VersionsOnly<'a> {
                #[serde(borrow, default)]
                versions: HashMap<String, &'a serde_json::value::RawValue>,
            }
            match serde_json::from_slice::<VersionsOnly>(&self.raw) {
                Ok(parsed) => parsed
                    .versions
                    .into_iter()
                    .map(|(version, raw)| (version, Box::from(raw.get())))
                    .collect(),
                Err(_) => HashMap::new(),
            }
        })
    }

    /// Extract a single version from raw bytes on demand.
    ///
    /// Looks the version's raw JSON up in the memoized per-version split (see
    /// [`version_blobs`](Self::version_blobs)) and deserializes only that small
    /// object into `T`. The whole-document structural pass happens at most once
    /// per manifest, so extracting K versions costs one split plus K small
    /// parses rather than K full-document re-parses.
    fn extract_version<T: for<'de> Deserialize<'de>>(&self, version: &str) -> Option<T> {
        let blob = self.version_blobs().get(version)?;
        serde_json::from_str(blob).ok()
    }

    /// Parse a single version on demand into CoreVersionManifest (hot path).
    ///
    /// Goes through the memoized per-version split, so repeated extractions of
    /// different versions from the same manifest share one structural pass.
    pub fn get_core_version(&self, version: &str) -> Option<CoreVersionManifest> {
        self.extract_version(version)
    }

    /// One-shot extraction for the single speculative extract performed while a
    /// full manifest is first parsed. That call resolves exactly one version and
    /// would never reuse the memoized split, so it must not pay to build (and
    /// retain) the per-version index — but it also must not pay a full-document
    /// parse: a shallow borrowed `RawValue` pass brace-matches past every other
    /// version without copying the body or materializing their objects (the
    /// previous `simd_json::to_borrowed_value` route cloned the whole multi-MB
    /// payload and parsed every version's subtree just to read one).
    pub fn get_core_version_oneshot(&self, version: &str) -> Option<CoreVersionManifest> {
        #[derive(Deserialize)]
        struct VersionsOnly<'a> {
            #[serde(borrow, default)]
            versions: HashMap<&'a str, &'a serde_json::value::RawValue>,
        }
        let parsed: VersionsOnly = serde_json::from_slice(&self.raw).ok()?;
        let blob = parsed.versions.get(version)?;
        serde_json::from_str(blob.get()).ok()
    }

    /// Parse a single version on demand into full VersionManifest (cold path, e.g. `ut view`).
    pub fn get_full_version(&self, version: &str) -> Option<VersionManifest> {
        self.extract_version(version)
    }

    /// Lazily parsed + descending-sorted version list (see
    /// [`parsed_versions`](Self::parsed_versions)).
    pub fn sorted_parsed_versions(&self) -> &[deno_semver::Version] {
        self.parsed_versions
            .get_or_init(|| sort_parsed_versions(&self.versions))
    }
}

/// Parse a version-string list and sort descending — shared by the lazy
/// per-package caches on [`FullManifest`] and `service::cache::VersionsInfo`.
pub(crate) fn sort_parsed_versions(versions: &[String]) -> Vec<deno_semver::Version> {
    let mut parsed: Vec<deno_semver::Version> = versions
        .iter()
        .filter_map(|v| deno_semver::Version::parse_from_npm(v).ok())
        .collect();
    parsed.sort_unstable_by(|a, b| b.cmp(a));
    parsed
}

/// Extract a single version from `FullManifest` on rayon's CPU pool
/// (native) or inline (wasm32). The native path keeps the tokio runtime
/// free of `simd_json::to_borrowed_value` work so sibling manifest
/// fetches keep driving network IO while this one re-parses.
///
/// Returns `(version, Option<Arc<CoreVersionManifest>>)` — the input
/// `version` is handed back to the caller alongside the result so it
/// can be reused for the error path and the outer return without a
/// clone. The native path requires `'static` for the rayon closure,
/// so the closure owns the string for its duration.
pub async fn extract_core_version_off_runtime(
    full: Arc<FullManifest>,
    version: String,
) -> (String, Option<Arc<CoreVersionManifest>>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        rayon::spawn(move || {
            let core = full.get_core_version(&version).map(Arc::new);
            let _ = tx.send((version, core));
        });
        rx.await.expect("rayon parse worker dropped before sending")
    }
    #[cfg(target_arch = "wasm32")]
    {
        let core = full.get_core_version(&version).map(Arc::new);
        (version, core)
    }
}

/// Deserialize a versions map by extracting only the keys, skipping all values.
///
/// Uses `IgnoredAny` to skip over version manifest JSON objects without allocating,
/// only collecting the version number strings.
fn deserialize_version_keys<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct VersionKeysVisitor;
    impl<'de> serde::de::Visitor<'de> for VersionKeysVisitor {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map of version strings to objects")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Vec<String>, M::Error> {
            let mut keys = Vec::with_capacity(map.size_hint().unwrap_or(0));
            while let Some(key) = map.next_key::<String>()? {
                map.next_value::<serde::de::IgnoredAny>()?;
                keys.push(key);
            }
            Ok(keys)
        }
    }
    deserializer.deserialize_map(VersionKeysVisitor)
}

/// Version-specific manifest from npm registry.
/// This represents a single version entry in the `versions` field.
///
/// The install-relevant fields live in the flattened [`CoreVersionManifest`]
/// (`core`) — the single source of truth for their names and `skip_on_error`
/// handling — and this struct only adds the display-oriented fields used by
/// `ut view`. The flatten round-trips through serde's content buffer, which
/// is fine on this cold path; the hot path parses [`CoreVersionManifest`]
/// directly and is unaffected.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionManifest {
    /// Install-relevant fields, shared with the hot path.
    #[serde(flatten)]
    pub core: CoreVersionManifest,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub main: Option<String>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<Repository>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub keywords: Option<Vec<String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<Author>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bugs: Option<Bugs>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub homepage: Option<String>,

    #[serde(
        rename = "bundledDependencies",
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bundled_dependencies: Option<Vec<String>>,

    #[serde(rename = "_id")]
    pub id: String,

    #[serde(rename = "_nodeVersion")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub node_version: Option<String>,

    #[serde(rename = "_npmVersion")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub npm_version: Option<String>,

    #[serde(rename = "_npmUser")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub npm_user: Option<NpmUser>,

    #[serde(rename = "_npmOperationalInternal")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_operational_internal: Option<NpmOperationalInternal>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub directories: Option<Directories>,
}

/// Slim version manifest for the hot path (resolution + lockfile + install).
///
/// Contains only the ~13 fields needed for dependency resolution, installation,
/// and lockfile serialization. Display-only fields (description, author, homepage,
/// keywords, bugs, repository, npm_user, etc.) are omitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreVersionManifest {
    pub name: String,
    pub version: String,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "devDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dev_dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "peerDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_dependencies: Option<HashMap<String, String>>,

    #[serde(rename = "optionalDependencies")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_dependencies: Option<HashMap<String, String>>,

    pub dist: Dist,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bin: Option<Value>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub engines: Option<HashMap<String, String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub os: Option<Value>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub cpu: Option<Value>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub scripts: Option<HashMap<String, String>>,

    #[serde(rename = "hasInstallScript")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub has_install_script: Option<bool>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub license: Option<String>,
}

/// Package author information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Author {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Repository information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Repository {
    #[serde(rename = "type")]
    pub repo_type: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// Bug tracker information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bugs {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Distribution information for a package version.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dist {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,

    #[serde(rename = "fileCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,

    #[serde(rename = "unpackedSize")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,

    #[serde(rename = "npm-signature")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_signature: Option<String>,
}

/// Package maintainer information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Maintainer {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// npm user information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmUser {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// npm operational internal metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NpmOperationalInternal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmp: Option<String>,
}

/// Directory paths in package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Directories {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub man: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
}

/// Manifest for a node in the dependency graph.
///
/// This enum distinguishes between local packages (root/workspace) and
/// registry packages (resolved dependencies).
#[derive(Debug, Clone)]
pub enum NodeManifest {
    /// Local package.json (root or workspace), boxed so the rare Local
    /// variant does not inflate every Registry node to PackageJson size.
    Local(Box<PackageJson>),
    /// Registry package manifest (resolved dependency, Arc-shared for cheap cloning)
    Registry(Arc<CoreVersionManifest>),
}

impl NodeManifest {
    /// Get the package name.
    pub fn name(&self) -> &str {
        match self {
            NodeManifest::Local(pkg) => &pkg.name,
            NodeManifest::Registry(manifest) => &manifest.name,
        }
    }

    /// Get the package version.
    pub fn version(&self) -> &str {
        match self {
            NodeManifest::Local(pkg) => &pkg.version,
            NodeManifest::Registry(manifest) => &manifest.version,
        }
    }

    /// Get production dependencies.
    pub fn dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.dependencies.as_ref(),
            NodeManifest::Registry(manifest) => manifest.dependencies.as_ref(),
        }
        .filter(|m| !m.is_empty())
    }

    /// Get peer dependencies.
    pub fn peer_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.peer_dependencies.as_ref(),
            NodeManifest::Registry(manifest) => manifest.peer_dependencies.as_ref(),
        }
        .filter(|m| !m.is_empty())
    }

    /// Get optional dependencies.
    pub fn optional_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.optional_dependencies.as_ref(),
            NodeManifest::Registry(manifest) => manifest.optional_dependencies.as_ref(),
        }
        .filter(|m| !m.is_empty())
    }

    /// Get dev dependencies (only for local packages).
    pub fn dev_dependencies(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.dev_dependencies.as_ref(),
            NodeManifest::Registry(_) => None,
        }
        .filter(|m| !m.is_empty())
    }

    /// Get engines requirements.
    /// Returns None for empty maps.
    pub fn engines(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.engines.as_ref(),
            NodeManifest::Registry(manifest) => manifest.engines.as_ref(),
        }
        .filter(|m| !m.is_empty())
    }

    /// Get binary configuration as Value (for serialization compatibility).
    /// Returns None for null or empty objects.
    pub fn bin(&self) -> Option<Value> {
        let value = match self {
            NodeManifest::Local(pkg) => pkg.bin.as_ref().and_then(|b| serde_json::to_value(b).ok()),
            NodeManifest::Registry(manifest) => manifest.bin.clone(),
        };
        // Filter out null and empty objects
        value.filter(|v| !v.is_null() && !v.as_object().is_some_and(|obj| obj.is_empty()))
    }

    /// Get license.
    pub fn license(&self) -> Option<String> {
        match self {
            NodeManifest::Local(pkg) => pkg.license.as_ref().map(|l| l.identifier().to_string()),
            NodeManifest::Registry(manifest) => manifest.license.clone(),
        }
    }

    /// Get OS constraints.
    pub fn os(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(pkg) => pkg.os.as_ref(),
            NodeManifest::Registry(manifest) => manifest.os.as_ref(),
        }
    }

    /// Get CPU constraints.
    pub fn cpu(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(pkg) => pkg.cpu.as_ref(),
            NodeManifest::Registry(manifest) => manifest.cpu.as_ref(),
        }
    }

    /// Check if has install script.
    pub fn has_install_script(&self) -> bool {
        match self {
            NodeManifest::Local(pkg) => pkg.has_install_script.unwrap_or(false),
            NodeManifest::Registry(manifest) => manifest.has_install_script.unwrap_or(false),
        }
    }

    /// Get scripts.
    pub fn scripts(&self) -> Option<&HashMap<String, String>> {
        match self {
            NodeManifest::Local(pkg) => pkg.scripts.as_ref(),
            NodeManifest::Registry(manifest) => manifest.scripts.as_ref(),
        }
        .filter(|m| !m.is_empty())
    }

    /// Get distribution info (tarball, integrity).
    pub fn dist(&self) -> Option<&Dist> {
        match self {
            NodeManifest::Local(_) => None,
            NodeManifest::Registry(manifest) => Some(&manifest.dist),
        }
    }

    /// Get workspaces configuration (only for local packages).
    pub fn workspaces(&self) -> Option<Value> {
        match self {
            NodeManifest::Local(pkg) => pkg
                .workspaces
                .as_ref()
                .and_then(|w| serde_json::to_value(w).ok()),
            NodeManifest::Registry(_) => None,
        }
    }

    /// Get overrides configuration (only for local packages).
    pub fn overrides(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(pkg) => pkg.overrides.as_ref(),
            NodeManifest::Registry(_) => None,
        }
    }
}

impl From<PackageJson> for NodeManifest {
    fn from(pkg: PackageJson) -> Self {
        NodeManifest::Local(Box::new(pkg))
    }
}

impl From<Arc<CoreVersionManifest>> for NodeManifest {
    fn from(manifest: Arc<CoreVersionManifest>) -> Self {
        NodeManifest::Registry(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version_from_memoized_split() {
        // A full manifest with two distinct versions carrying different deps.
        let raw = br#"{
            "name": "pkg",
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "1.0.0": { "name": "pkg", "version": "1.0.0", "dependencies": { "a": "^1" } },
                "2.0.0": { "name": "pkg", "version": "2.0.0", "dependencies": { "b": "^2" } }
            }
        }"#;
        let mut manifest: FullManifest = serde_json::from_slice(raw).unwrap();
        manifest.raw = Bytes::from_static(raw);

        // Each version resolves to its own deps, and the memoized split is
        // reused across the two extractions (only one structural pass).
        let v1 = manifest.get_core_version("1.0.0").expect("1.0.0 present");
        assert_eq!(v1.version, "1.0.0");
        assert!(v1.dependencies.as_ref().unwrap().contains_key("a"));

        let v2 = manifest.get_core_version("2.0.0").expect("2.0.0 present");
        assert_eq!(v2.version, "2.0.0");
        assert!(v2.dependencies.as_ref().unwrap().contains_key("b"));

        // Missing version → None, not a panic.
        assert!(manifest.get_core_version("9.9.9").is_none());

        // The one-shot speculative path agrees with the memoized split.
        let one = manifest
            .get_core_version_oneshot("2.0.0")
            .expect("2.0.0 present");
        assert_eq!(one.version, v2.version);
        assert_eq!(one.dependencies, v2.dependencies);
    }

    #[test]
    fn test_author_string_deserialization() {
        let json = r#"{"author": "Erik Lieben <https://github.com/eriklieben>"}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.author.is_none());
    }

    #[test]
    fn test_author_object_deserialization() {
        let json = r#"{"author": {"name": "Erik Lieben", "email": "erik@example.com", "url": "https://github.com/eriklieben"}}"#;

        #[derive(Deserialize)]
        struct TestManifest {
            #[serde(deserialize_with = "skip_on_error")]
            pub author: Option<Author>,
        }

        let manifest: TestManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.author.is_some());
        let author = manifest.author.unwrap();
        assert_eq!(author.name, "Erik Lieben");
        assert_eq!(author.email, Some("erik@example.com".to_string()));
    }

    #[test]
    fn test_manifest_with_serde_default() {
        let json = r#"{"name": "test-package"}"#;
        let manifest: FullManifest = serde_json::from_str(json).unwrap();

        assert_eq!(manifest.name, "test-package");
        assert_eq!(manifest.description, None);
        assert!(manifest.dist_tags.is_empty());
        assert!(manifest.versions.is_empty());
    }

    #[test]
    fn test_version_manifest_parsing() {
        let json = r#"{
            "name": "jsonparse",
            "version": "1.3.1",
            "license": "MIT",
            "dependencies": { "lodash": "^4.0.0" }
        }"#;

        let manifest: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.core.name, "jsonparse");
        assert_eq!(manifest.core.version, "1.3.1");
        assert_eq!(manifest.core.license, Some("MIT".to_string()));
        assert!(manifest.core.dependencies.is_some());
    }

    #[test]
    fn test_license_array_does_not_break_parsing() {
        // Some old packages use "license" as an array of objects instead of a string
        let json = r#"{
            "name": "xss",
            "version": "0.2.0",
            "license": [{"type":"MIT","url":"https://example.com/MIT-License"}],
            "dist": {"tarball":"https://registry.npmjs.org/xss/-/xss-0.2.0.tgz","shasum":"abc123"}
        }"#;

        let manifest: CoreVersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "xss");
        assert_eq!(manifest.version, "0.2.0");
        assert_eq!(manifest.license, None); // skip_on_error should handle this

        let manifest2: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest2.core.license, None);
    }

    #[test]
    fn test_licenses_plural_field_does_not_break_parsing() {
        // "licenses" (plural array) instead of "license" (string) should be silently ignored
        let json = r#"{
            "name": "legacy-pkg",
            "version": "0.1.20",
            "description": "A package with legacy licenses field",
            "dependencies": {"commander": "2.1.x"},
            "devDependencies": {"mocha": "1.8.2"},
            "licenses": [{"type":"MIT","url":"https://opensource.org/licenses/MIT"}],
            "dist": {
                "shasum": "539f38e2427e37e6fa13cd417f98f644d2d0c4a6",
                "tarball": "https://registry.npmjs.org/legacy-pkg/-/legacy-pkg-0.1.20.tgz",
                "integrity": "sha512-f4BQyF9YKKI7Y5O"
            },
            "bin": {"legacy": "./bin/legacy"}
        }"#;

        let manifest: CoreVersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "legacy-pkg");
        assert_eq!(manifest.version, "0.1.20");
        assert_eq!(manifest.license, None); // "licenses" is unknown field, ignored
        assert!(manifest.dependencies.is_some());
        assert_eq!(
            manifest.dependencies.as_ref().unwrap().get("commander"),
            Some(&"2.1.x".to_string())
        );
    }
}
