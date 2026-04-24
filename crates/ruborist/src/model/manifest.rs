//! npm registry manifest types.
//!
//! These types represent the JSON responses from npm registry API.
//! Used by both PM (native) and WASM (browser) implementations.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

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

    /// Version keys (preserved order) and pre-parsed `CoreVersionManifest`s,
    /// populated in a single pass by [`Versions`]'s custom `Deserialize`.
    ///
    /// Replaces the previous `versions: Vec<String>` (populated by
    /// `IgnoredAny` visitor) + per-resolve `extract_version` that
    /// re-parsed the full raw JSON via `simd_json::to_borrowed_value`
    /// for every `resolve_package` call. That re-parse was uninstrumented
    /// CPU on the async worker — ~0.4ms avg × 4567 resolves = 1.85s
    /// serial load on the 4-core tokio runtime, preventing the pipeline
    /// from maintaining 64 in-flight fetches (observed ~38 effective).
    ///
    /// With eager parse, `get_core_version` becomes an O(1) map lookup.
    pub versions: Versions,

    /// Raw HTTP response bytes. Retained only for `get_full_version`
    /// (cold path — `ut view` command) which still needs on-demand
    /// extraction of the full `VersionManifest`. Hot-path resolve uses
    /// [`Self::versions`] which is pre-parsed.
    #[serde(skip)]
    pub raw: Arc<[u8]>,

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
    /// On-demand `CoreVersionManifest` extraction from a pre-parsed
    /// `simd_json::OwnedValue` subtree.
    ///
    /// History: we've been through three designs here.
    /// 1. **Eager struct parse (previous)**: `Versions::deserialize`
    ///    built `Arc<CoreVersionManifest>` for every version up
    ///    front. Lookup was O(1) but fetch-time spawn_blocking had to
    ///    materialise ~500 structs per manifest × 2730 manifests =
    ///    ~1.37M CoreVersionManifest builds, 99 % of which the
    ///    resolver never reads. Blocking pool saturation at cap=128
    ///    showed up as `parse_us` avg 20 ms with long tails.
    /// 2. **Lazy `Arc<serde_json::Value>` (tried in c5cd8318,
    ///    reverted fe8365d7)**: stored `serde_json::Value` subtrees
    ///    and called `from_value(value.clone())` on demand. The
    ///    deep clone + serde_json walk ran on async worker and
    ///    measured `core_version_us` 11-18 ms avg — worse than eager.
    /// 3. **This design — lazy simd_json `OwnedValue` + memoisation**:
    ///    Store each version as `Arc<simd_json::OwnedValue>` (the
    ///    native simd_json tree form — no serde_json round-trip).
    ///    On demand call `from_refvalue(&OwnedValue)` which
    ///    zero-copies through `&Value` implementing `Deserializer`.
    ///    First hit for a given (manifest, version) pays ~50-200 μs
    ///    of subtree walking; subsequent hits are `Arc::clone` via
    ///    the `DashMap` memoisation cache.
    ///
    /// Why this is better than design 1: a typical resolve touches
    /// 1-3 of a manifest's ~500 versions. Building the other 497+
    /// was pure waste on the blocking pool's critical path.
    ///
    /// Why this is better than design 2: `simd_json::OwnedValue`'s
    /// `&Value: Deserializer` zero-copies through the tree without
    /// allocating an intermediate `serde_json::Value`. And moving
    /// the conversion out of `spawn_blocking` (it's small, runs on
    /// async worker) removes the 200 μs dispatch overhead that was
    /// ~500 ms of the previous lazy attempt.
    pub fn get_core_version(&self, version: &str) -> Option<Arc<CoreVersionManifest>> {
        // Fast path: memoised conversion.
        if let Some(cached) = self.versions.cache.get(version) {
            return Some(cached.clone());
        }
        let tree = self.versions.trees.get(version)?;
        // `&simd_json::OwnedValue` impls `Deserializer<'_>` directly
        // (see simd_json::serde::value::owned::de), so this is a
        // zero-allocation tree walk — no Value clone, no bytes copy.
        let core = CoreVersionManifest::deserialize(tree.as_ref()).ok()?;
        let arc = Arc::new(core);
        self.versions.cache.insert(version.to_string(), arc.clone());
        Some(arc)
    }

    /// Parse a single version on demand into full `VersionManifest`
    /// (cold path — `ut view`). Uses the retained raw bytes because
    /// `VersionManifest` carries display fields that `CoreVersionManifest`
    /// drops, and `ut view` is infrequent.
    pub fn get_full_version(&self, version: &str) -> Option<VersionManifest> {
        use simd_json::prelude::ValueObjectAccess;
        let mut buf = self.raw.to_vec();
        let parsed = simd_json::to_borrowed_value(&mut buf).ok()?;
        let version_obj = parsed.get("versions")?.get(version)?;
        let value = serde_json::to_value(version_obj).ok()?;
        serde_json::from_value(value).ok()
    }
}

/// Version entries of a `FullManifest`: preserves key insertion order
/// (for `VersionsInfo`/semver callers that iterate the list) alongside
/// per-version pre-parsed JSON subtrees and a memoisation cache for
/// strongly-typed `CoreVersionManifest` conversions.
///
/// Fetch-time parse builds only `Arc<simd_json::OwnedValue>` per
/// version — cheaper than constructing a full `CoreVersionManifest`
/// because `OwnedValue` is the native simd_json tree without
/// field-by-field `#[serde(default)]` / `skip_on_error` validation.
/// The real `CoreVersionManifest` is materialised on demand inside
/// `FullManifest::get_core_version`, so the ~99 % of versions the
/// resolver never touches don't pay for field-level parsing.
#[derive(Debug, Clone, Default)]
pub struct Versions {
    /// Version strings in insertion order — used by
    /// `resolve_target_version` and cached into `VersionsInfo`.
    pub keys: Vec<String>,
    /// Per-version pre-parsed JSON subtrees. `Arc` because the
    /// `FullManifest` is cloned across cache tiers (memory / project
    /// cache) and we want Arc-level sharing of the underlying bytes.
    pub trees: HashMap<String, Arc<simd_json::OwnedValue>>,
    /// Memoisation cache populated on first `get_core_version(v)`
    /// call. `DashMap` instead of `Mutex<HashMap>` because
    /// multiple resolves for the same (name, version) can land
    /// concurrently on different async workers; DashMap avoids
    /// serialising them on a global mutex.
    pub cache: Arc<dashmap::DashMap<String, Arc<CoreVersionManifest>>>,
}

impl Versions {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl Serialize for Versions {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize the same shape the registry returns: a JSON object
        // mapping version string -> raw JSON tree. Order follows
        // `keys` for determinism (HashMap iteration order is arbitrary
        // otherwise).
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.keys.len()))?;
        for k in &self.keys {
            if let Some(v) = self.trees.get(k) {
                map.serialize_entry(k, v.as_ref())?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Versions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct VersionsVisitor;
        impl<'de> serde::de::Visitor<'de> for VersionsVisitor {
            type Value = Versions;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map of version strings to manifest objects")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Versions, M::Error> {
                let cap = map.size_hint().unwrap_or(0);
                let mut keys = Vec::with_capacity(cap);
                let mut trees = HashMap::with_capacity(cap);
                while let Some(key) = map.next_key::<String>()? {
                    // Deserialize into `simd_json::OwnedValue` — the
                    // native simd_json tree. Cheaper than building
                    // `CoreVersionManifest` because it skips field
                    // validation / `skip_on_error` round-trips; those
                    // only run on demand for versions the resolver
                    // actually reads.
                    let tree: simd_json::OwnedValue = map.next_value()?;
                    trees.insert(key.clone(), Arc::new(tree));
                    keys.push(key);
                }
                Ok(Versions {
                    keys,
                    trees,
                    cache: Arc::new(dashmap::DashMap::new()),
                })
            }
        }
        deserializer.deserialize_map(VersionsVisitor)
    }
}

/// Version-specific manifest from npm registry.
/// This represents a single version entry in the `versions` field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionManifest {
    pub name: String,
    pub version: String,

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
    pub scripts: Option<HashMap<String, String>>,

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
    pub license: Option<String>,

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

    #[serde(
        rename = "bundledDependencies",
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bundled_dependencies: Option<Vec<String>>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub engines: Option<HashMap<String, String>>,

    /// Binary files configuration - can be string or object
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bin: Option<Value>,

    /// Install script indicator (used by npm to optimize package installation)
    #[serde(rename = "hasInstallScript")]
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub has_install_script: Option<bool>,

    /// Platform compatibility - CPU
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub cpu: Option<Value>,

    /// Platform compatibility - OS
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub os: Option<Value>,

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

    pub dist: Dist,

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
///
/// `skip_on_error` is kept on every field that's typed as `HashMap`,
/// `String`, or `bool` — real registry data regularly ships values
/// that don't match the expected shape (e.g. `engines` with a null
/// value, `hasInstallScript` as a string, legacy `license` as an
/// array-of-object). Dropping the wrapper on those fields broke the
/// lodash fetch at `"ExpectedMap at character 0"` in CI.
///
/// The three `Option<Value>` fields (`bin`, `os`, `cpu`) are the only
/// safe cases: `Value` absorbs any JSON, so the `Value::deserialize`
/// round-trip in `skip_on_error` provides zero robustness. Dropping
/// it there saves one allocation + recursive walk per occurrence
/// without correctness risk.
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bin: Option<Value>,

    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub engines: Option<HashMap<String, String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
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

/// Simplified package manifest (for `npm view` output).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub homepage: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub license: Option<String>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub keywords: Option<Vec<String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<Author>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<Repository>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub bugs: Option<Bugs>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dist: Option<Dist>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub maintainers: Option<Vec<Maintainer>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub dist_tags: Option<HashMap<String, String>>,
    #[serde(
        deserialize_with = "skip_on_error",
        skip_serializing_if = "Option::is_none"
    )]
    pub versions: Option<HashMap<String, VersionInfo>>,
    pub versions_count: usize,
}

/// Simplified version info.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct VersionInfo {
    pub publish_time: Option<u64>,
    #[serde(rename = "_npmUser")]
    pub npm_user: Option<NpmUser>,
}

use super::package_json::PackageJson;

/// Manifest for a node in the dependency graph.
///
/// This enum distinguishes between local packages (root/workspace) and
/// registry packages (resolved dependencies).
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum NodeManifest {
    /// Local package.json (root or workspace)
    Local(PackageJson),
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
            NodeManifest::Local(_) => None, // PackageJson uses Vec<String>
            NodeManifest::Registry(manifest) => manifest.os.as_ref(),
        }
    }

    /// Get CPU constraints.
    pub fn cpu(&self) -> Option<&Value> {
        match self {
            NodeManifest::Local(_) => None, // PackageJson uses Vec<String>
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
        NodeManifest::Local(pkg)
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
        assert_eq!(manifest.name, "jsonparse");
        assert_eq!(manifest.version, "1.3.1");
        assert_eq!(manifest.license, Some("MIT".to_string()));
        assert!(manifest.dependencies.is_some());
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
        assert_eq!(manifest2.license, None);
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
