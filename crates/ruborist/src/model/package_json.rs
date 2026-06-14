//! Package.json model type for ruborist.
//!
//! This module provides strongly-typed representations of package.json content,
//! avoiding the need to pass raw `serde_json::Value` through the API.

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

use super::compatibility::PlatformConstraint;
use super::util::deserialize_or_default;

/// Deserialize a `scripts` map, silently dropping entries whose values are not
/// strings (e.g. `"blanket": {"pattern": "xss/lib"}`).
fn deserialize_string_map<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: HashMap<String, Value> = HashMap::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_owned())))
        .collect())
}

/// Parsed package.json content.
///
/// This is the primary type for representing package.json in ruborist.
/// It contains all fields needed for dependency resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageJson {
    /// Package name
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,

    /// Package version
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,

    /// Production dependencies
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, String>>,

    /// Development dependencies
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_dependencies: Option<HashMap<String, String>>,

    /// Peer dependencies
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_dependencies: Option<HashMap<String, String>>,

    /// Optional dependencies
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_dependencies: Option<HashMap<String, String>>,

    /// Overrides (npm) / resolutions (yarn)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Value>,

    /// Workspaces configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspaces: Option<WorkspacesConfig>,

    /// Engine requirements
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_or_default"
    )]
    pub engines: Option<HashMap<String, String>>,

    /// Binary definitions (string or object). Same typed wire shape as
    /// `CoreVersionManifest`/`LockPackage`; malformed values read as `None`
    /// (= no binaries), matching npm.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_or_default"
    )]
    pub bin: Option<BinField>,

    /// Package license
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<LicenseConfig>,

    /// Scripts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<HashMap<String, String>>,

    /// OS constraints. Same typed wire shape as
    /// `CoreVersionManifest`/`LockPackage`; malformed values read as `None`
    /// (= unconstrained), matching npm.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_or_default"
    )]
    pub os: Option<PlatformConstraint>,

    /// CPU constraints (same string-or-array tolerance as `os`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_or_default"
    )]
    pub cpu: Option<PlatformConstraint>,

    /// Has install scripts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_install_script: Option<bool>,

    /// Distribution info (from registry)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist: Option<crate::model::manifest::Dist>,

    /// Package description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether package is private
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,

    /// Files whitelist for publishing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,

    /// Main entry point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,

    /// TypeScript types entry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<String>,

    /// TypeScript typings entry (legacy alias for types)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typings: Option<String>,

    /// Publish configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_config: Option<PublishConfig>,

    /// Preserve other fields we don't explicitly model
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Workspaces configuration (can be array or object with packages field).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkspacesConfig {
    /// Simple array of glob patterns
    Array(Vec<String>),
    /// Object with packages field
    Object { packages: Vec<String> },
}

impl WorkspacesConfig {
    /// Get the workspace patterns.
    pub fn patterns(&self) -> &[String] {
        match self {
            WorkspacesConfig::Array(patterns) => patterns,
            WorkspacesConfig::Object { packages } => packages,
        }
    }
}

// ── Lightweight views ───────────────────────────────────────────────
// Each view only declares the fields it needs; serde silently skips the
// rest, so non-standard fields in third-party packages cannot break the
// parse.

/// Minimal view for rebuild: only lifecycle scripts.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScriptsView {
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub scripts: HashMap<String, String>,
}

/// Minimal view for cache validation: name + version.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct IdentityView {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// View for post-install processing (rebuild / bin linking).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PackageInstallView {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub bin: Option<BinField>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub scripts: HashMap<String, String>,
}

impl PackageInstallView {
    /// Get binary entries as (name, path) pairs.
    pub fn bin_entries(&self) -> Vec<(String, String)> {
        self.bin
            .as_ref()
            .map(|bin| bin.entries(&self.name))
            .unwrap_or_default()
    }
}

/// View for lockfile freshness check: dependency maps only.
///
/// Does not include `engines` — that is root-only and loaded separately
/// via [`EnginesView`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepsView {
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, String>,
    #[serde(default)]
    pub peer_dependencies: HashMap<String, String>,
    #[serde(default)]
    pub optional_dependencies: HashMap<String, String>,
}

/// View for root-only engine requirements.
///
/// Used alongside [`DepsView`] for lockfile freshness checks — engines
/// are only relevant for the root package, not workspace members.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnginesView {
    #[serde(default, deserialize_with = "deserialize_or_default")]
    pub engines: Option<HashMap<String, String>>,
}

/// npm's polymorphic `bin` field, as it appears on every wire (package.json,
/// registry version manifests, lockfiles).
///
/// npm's grammar is exactly two shapes: a bare string (the package exposes one
/// binary named after itself) or a name→path map. This untagged enum captures
/// exactly those forms; anything else fails deserialization, which carrier
/// fields map to `None` (= no binaries).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinField {
    /// `"bin": "./cli.js"` — one binary named after the package.
    Single(String),
    /// `"bin": {"name": "./path"}` — explicit name→path map. `BTreeMap`, not
    /// `HashMap`: this round-trips into package-lock.json, so key order must be
    /// deterministic (a multi-binary package like `typescript` → tsc/tsserver
    /// must serialize identically every run).
    Map(BTreeMap<String, String>),
}

impl BinField {
    /// Get binary entries as (name, path) pairs, dropping entries with an empty
    /// path. For [`BinField::Single`] the binary is named after `package_name`.
    pub fn entries(&self, package_name: &str) -> Vec<(String, String)> {
        match self {
            BinField::Single(path) => vec![(package_name.to_string(), path.clone())],
            BinField::Map(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
        .into_iter()
        .filter(|(_, path)| !path.is_empty())
        .collect()
    }
}

/// License configuration (can be string or object).
///
/// The on-disk package.json shape of npm's polymorphic license field; see
/// [`super::package_lock::License`] for the lockfile shape and how the three
/// representations divide the wire contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LicenseConfig {
    /// SPDX license identifier
    String(String),
    /// Object with type and url
    Object { r#type: String, url: Option<String> },
}

impl LicenseConfig {
    /// Get the license identifier.
    pub fn identifier(&self) -> &str {
        match self {
            LicenseConfig::String(s) => s,
            LicenseConfig::Object { r#type, .. } => r#type,
        }
    }
}

/// Publish configuration from package.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PublishConfig {
    /// Distribution tag (e.g., "latest", "beta")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// Registry URL override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,

    /// Package access level ("public" or "restricted")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<String>,
}

/// Empty map constant for convenience methods that return references.
static EMPTY_MAP: std::sync::LazyLock<HashMap<String, String>> =
    std::sync::LazyLock::new(HashMap::new);

impl PackageJson {
    /// Create a new empty PackageJson.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    /// Parse from JSON Value.
    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value.clone())
    }

    /// Convert to JSON Value.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Get dependencies, defaulting to empty map.
    pub fn deps(&self) -> &HashMap<String, String> {
        self.dependencies.as_ref().unwrap_or(&EMPTY_MAP)
    }

    /// Get dev dependencies, defaulting to empty map.
    pub fn dev_deps(&self) -> &HashMap<String, String> {
        self.dev_dependencies.as_ref().unwrap_or(&EMPTY_MAP)
    }

    /// Get peer dependencies, defaulting to empty map.
    pub fn peer_deps(&self) -> &HashMap<String, String> {
        self.peer_dependencies.as_ref().unwrap_or(&EMPTY_MAP)
    }

    /// Get optional dependencies, defaulting to empty map.
    pub fn optional_deps(&self) -> &HashMap<String, String> {
        self.optional_dependencies.as_ref().unwrap_or(&EMPTY_MAP)
    }

    /// Get scripts, defaulting to empty map.
    pub fn scripts_or_empty(&self) -> &HashMap<String, String> {
        self.scripts.as_ref().unwrap_or(&EMPTY_MAP)
    }

    /// Check if package has any dependencies.
    pub fn has_dependencies(&self) -> bool {
        !self.deps().is_empty()
            || !self.dev_deps().is_empty()
            || !self.peer_deps().is_empty()
            || !self.optional_deps().is_empty()
    }

    /// Get all dependency types as iterator.
    pub fn all_dependencies(&self) -> impl Iterator<Item = (&str, &HashMap<String, String>)> {
        [
            ("dependencies", self.deps()),
            ("devDependencies", self.dev_deps()),
            ("peerDependencies", self.peer_deps()),
            ("optionalDependencies", self.optional_deps()),
        ]
        .into_iter()
        .filter(|(_, deps)| !deps.is_empty())
    }

    /// Get tarball URL from dist info.
    pub fn tarball_url(&self) -> Option<&str> {
        self.dist.as_ref().and_then(|d| d.tarball.as_deref())
    }

    /// Get integrity hash from dist info.
    pub fn integrity(&self) -> Option<&str> {
        self.dist.as_ref().and_then(|d| d.integrity.as_deref())
    }

    /// Get binary entries as (name, path) pairs.
    pub fn bin_entries(&self) -> Vec<(String, String)> {
        self.bin
            .as_ref()
            .map(|bin| bin.entries(&self.name))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// npm's spec says os/cpu are string arrays, but the CLI tolerates bare
    /// strings and real packages ship both — parsing must accept both shapes
    /// (a string-form `os` used to fail the whole PackageJson parse).
    #[test]
    fn test_os_cpu_accept_string_and_array() {
        let array_form = json!({
            "name": "a", "version": "1.0.0",
            "os": ["!win32"], "cpu": ["x64", "arm64"]
        });
        let pkg = PackageJson::from_value(&array_form).unwrap();
        assert_eq!(
            pkg.os,
            Some(PlatformConstraint::Many(vec!["!win32".to_string()]))
        );

        let string_form = json!({
            "name": "b", "version": "1.0.0",
            "os": "linux", "cpu": "x64"
        });
        let pkg = PackageJson::from_value(&string_form).unwrap();
        assert_eq!(pkg.os, Some(PlatformConstraint::One("linux".to_string())));
        assert_eq!(pkg.cpu, Some(PlatformConstraint::One("x64".to_string())));
    }

    #[test]
    fn test_parse_simple_package() {
        let value = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.0"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.name, "test-package");
        assert_eq!(pkg.version, "1.0.0");
        assert_eq!(pkg.deps().get("lodash"), Some(&"^4.17.0".to_string()));
    }

    #[test]
    fn test_parse_workspaces_array() {
        let value = json!({
            "name": "monorepo",
            "workspaces": ["packages/*"]
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let workspaces = pkg.workspaces.unwrap();
        let patterns = workspaces.patterns();
        assert_eq!(patterns, &["packages/*"]);
    }

    #[test]
    fn test_parse_workspaces_object() {
        let value = json!({
            "name": "monorepo",
            "workspaces": {
                "packages": ["packages/*", "apps/*"]
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let workspaces = pkg.workspaces.unwrap();
        let patterns = workspaces.patterns();
        assert_eq!(patterns, &["packages/*", "apps/*"]);
    }

    #[test]
    fn test_parse_bin_string() {
        let value = json!({
            "name": "my-cli",
            "bin": "./cli.js"
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(
            pkg.bin_entries(),
            vec![("my-cli".to_string(), "./cli.js".to_string())]
        );
    }

    #[test]
    fn test_parse_bin_map() {
        // Single key with custom name
        let value = json!({
            "name": "my-cli",
            "bin": {
                "a": "./index.js"
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(
            pkg.bin_entries(),
            vec![("a".to_string(), "./index.js".to_string())]
        );

        // Multiple keys
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "tool2": "./bin/tool2.js"
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin_entries();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|(k, v)| k == "tool1" && v == "./bin/tool1.js")
        );
        assert!(
            entries
                .iter()
                .any(|(k, v)| k == "tool2" && v == "./bin/tool2.js")
        );
    }

    #[test]
    fn test_parse_bin_empty_filtered() {
        // Empty bin string should be filtered out
        let value = json!({
            "name": "my-cli",
            "bin": ""
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.bin_entries().len(), 0);

        // Empty bin in map should be filtered out
        let value = json!({
            "name": "my-tools",
            "bin": {
                "tool1": "./bin/tool1.js",
                "empty": ""
            }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        let entries = pkg.bin_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "tool1");
    }

    #[test]
    fn test_bin_field_entries() {
        // String form names the binary after the package.
        let single = BinField::Single("./cli.js".to_string());
        assert_eq!(
            single.entries("my-cli"),
            vec![("my-cli".to_string(), "./cli.js".to_string())]
        );

        // Map form keeps its own names.
        let map = BinField::Map(BTreeMap::from([(
            "a".to_string(),
            "./index.js".to_string(),
        )]));
        assert_eq!(
            map.entries("ignored"),
            vec![("a".to_string(), "./index.js".to_string())]
        );

        // Empty paths are dropped in both forms.
        assert!(BinField::Single(String::new()).entries("my-cli").is_empty());
        let map = BinField::Map(BTreeMap::from([
            ("tool".to_string(), "./bin/tool.js".to_string()),
            ("empty".to_string(), String::new()),
        ]));
        let entries = map.entries("ignored");
        assert_eq!(
            entries,
            vec![("tool".to_string(), "./bin/tool.js".to_string())]
        );
    }

    #[test]
    fn test_bin_non_string_value_tolerated() {
        // A map with a non-string value is not representable as BinField; the
        // whole parse must still succeed with bin = None (npm coerces such a
        // value to "" then drops it — same observable result: no binaries).
        let value = json!({
            "name": "my-cli",
            "bin": { "x": 123 }
        });
        let pkg = PackageJson::from_value(&value).unwrap();
        assert!(pkg.bin.is_none());
        assert!(pkg.bin_entries().is_empty());
    }

    #[test]
    fn test_parse_dist_info() {
        let value = json!({
            "name": "lodash",
            "version": "4.17.21",
            "dist": {
                "tarball": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                "integrity": "sha512-xyz"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(
            pkg.tarball_url(),
            Some("https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz")
        );
        assert_eq!(pkg.integrity(), Some("sha512-xyz"));
    }

    #[test]
    fn test_parse_publish_fields() {
        let value = json!({
            "name": "my-pkg",
            "version": "1.0.0",
            "description": "A great package",
            "private": true,
            "files": ["dist", "lib"],
            "main": "./lib/index.js",
            "types": "./lib/index.d.ts",
            "typings": "./lib/index.d.ts",
            "publishConfig": {
                "tag": "beta",
                "registry": "https://custom.registry.org",
                "access": "public"
            }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.description.as_deref(), Some("A great package"));
        assert_eq!(pkg.private, Some(true));
        assert_eq!(
            pkg.files.as_deref(),
            Some(&["dist".to_string(), "lib".to_string()][..])
        );
        assert_eq!(pkg.main.as_deref(), Some("./lib/index.js"));
        assert_eq!(pkg.types.as_deref(), Some("./lib/index.d.ts"));
        assert_eq!(pkg.typings.as_deref(), Some("./lib/index.d.ts"));

        let pc = pkg.publish_config.as_ref().unwrap();
        assert_eq!(pc.tag.as_deref(), Some("beta"));
        assert_eq!(pc.registry.as_deref(), Some("https://custom.registry.org"));
        assert_eq!(pc.access.as_deref(), Some("public"));

        // Round-trip: ensure new fields survive to_value
        let rt = pkg.to_value();
        assert_eq!(rt["description"], "A great package");
        assert_eq!(rt["private"], true);
        assert_eq!(rt["main"], "./lib/index.js");
        assert_eq!(rt["publishConfig"]["tag"], "beta");
    }

    #[test]
    fn test_engines_non_standard_array_format() {
        // ansi-html-community uses `"engines": ["node >= 0.8.0"]` (array instead of map).
        // This should NOT fail the entire parse; engines should default to None.
        let value = json!({
            "name": "ansi-html-community",
            "version": "0.0.8",
            "engines": ["node >= 0.8.0"]
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        assert_eq!(pkg.name, "ansi-html-community");
        assert!(pkg.engines.is_none());
    }

    #[test]
    fn test_engines_standard_map_format() {
        let value = json!({
            "name": "some-package",
            "version": "1.0.0",
            "engines": { "node": ">=18" }
        });

        let pkg = PackageJson::from_value(&value).unwrap();
        let engines = pkg.engines.unwrap();
        assert_eq!(engines.get("node"), Some(&">=18".to_string()));
    }

    #[test]
    fn test_roundtrip() {
        let original = json!({
            "name": "test",
            "version": "1.0.0",
            "dependencies": { "lodash": "^4.0.0" },
            "devDependencies": { "jest": "^29.0.0" },
            "scripts": { "test": "jest" }
        });

        let pkg = PackageJson::from_value(&original).unwrap();
        let value = pkg.to_value();

        assert_eq!(value["name"], "test");
        assert_eq!(value["version"], "1.0.0");
        assert_eq!(value["dependencies"]["lodash"], "^4.0.0");
    }
}
