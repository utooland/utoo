//! Catalog protocol support for `.utoo.toml`.
//!
//! Reads `[catalog]` (default) and `[catalogs.<name>]` (named) sections
//! from `.utoo.toml` and returns them as a unified `Catalogs` map
//! that ruborist uses to resolve `catalog:` dependency specifiers.
//!
//! ## .utoo.toml format
//!
//! ```toml
//! [catalog]
//! lodash = "^4.17.21"
//! react = "^18.0.0"
//!
//! [catalogs.legacy]
//! path-to-regexp = "^1.9.0"
//! ```
//!
//! ## Catalog resolution flow
//!
//! ```text
//!  utoo install
//!       |
//!       v
//!  +--------------------------+
//!  | load_catalogs()          |  <-- reads .utoo.toml from project root
//!  | (pm/helper/catalog.rs)   |      returns Catalogs { "" => default, "name" => named }
//!  +--------------------------+
//!       |
//!       |  Catalogs passed into BuildDepsOptions
//!       v
//!  +--------------------------+     +---------------------------------+
//!  | build_deps()             |     | is_pkg_lock_outdated()          |
//!  | (ruborist/service/api.rs)|     | (pm/helper/lock.rs)             |
//!  |  for each pkg:           |     |  pkg.resolve_catalogs()         |
//!  |    pkg.resolve_catalogs()|     |  before comparing with lockfile |
//!  +--------------------------+     +---------------------------------+
//!       |
//!       |  catalog: specs replaced with real semver ranges
//!       v
//!  +--------------------------+
//!  | Registry resolution      |  <-- normal dep graph construction
//!  | (no catalog: refs remain)|      with resolved version strings
//!  +--------------------------+
//! ```
//!
//! ## Specifier forms
//!
//! | Form              | Resolves to           |
//! |-------------------|-----------------------|
//! | `catalog:`        | default catalog (`""`) |
//! | `catalog:default` | default catalog (`""`) |
//! | `catalog:<name>`  | named catalog          |

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use utoo_ruborist::spec::Catalogs;

use crate::util::config_file::Config;

/// Model for the catalog-related sections of .utoo.toml.
///
/// Uses `#[serde(default)]` so missing sections are silently ignored,
/// and `#[serde(flatten)]` is NOT used -- we only extract the catalog
/// fields; all other .utoo.toml keys are ignored via deny_unknown_fields=false.
#[derive(Debug, Default, Deserialize)]
struct CatalogConfig {
    /// Default catalog: `[catalog]` section.
    #[serde(default)]
    catalog: HashMap<String, toml::Value>,

    /// Named catalogs: `[catalogs.<name>]` sections.
    #[serde(default)]
    catalogs: HashMap<String, HashMap<String, toml::Value>>,
}

/// Convert a toml::Value to a String.
///
/// TOML may parse unquoted integer-like values as integers, so we need to
/// handle both string and numeric types.
fn toml_value_to_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(n) => Some(n.to_string()),
        other => {
            tracing::warn!(
                "unsupported TOML value type for catalog entry: {}",
                other.type_str()
            );
            None
        }
    }
}

fn toml_map_to_string_map(map: &HashMap<String, toml::Value>) -> HashMap<String, String> {
    map.iter()
        .filter_map(|(k, v)| toml_value_to_string(v).map(|s| (k.clone(), s)))
        .collect()
}

/// Load catalog definitions from `.utoo.toml` in the given directory.
///
/// Uses cached `.utoo.toml` content from `Config::init_local()` when
/// available, falling back to a direct file read otherwise.
///
/// Returns an empty map if the file doesn't exist, can't be parsed, or
/// contains no catalog sections.  The default catalog is stored under
/// key `""` (empty string).
pub fn load_catalogs(root_path: &Path) -> Catalogs {
    // Use cached .utoo.toml content from Config::init_local() if available
    if let Some(content) = Config::local_content() {
        return parse_catalogs(content);
    }

    // Fallback: read from disk (Config::init_local not called yet, e.g. in tests)
    let toml_path = root_path.join(".utoo.toml");
    match std::fs::read_to_string(&toml_path) {
        Ok(content) => parse_catalogs(&content),
        Err(_) => HashMap::new(),
    }
}

/// Parse catalog definitions from raw `.utoo.toml` content.
fn parse_catalogs(content: &str) -> Catalogs {
    let config: CatalogConfig = match toml::from_str(content) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to parse .utoo.toml: {}", e);
            return HashMap::new();
        }
    };

    let mut catalogs: Catalogs = HashMap::new();

    // Default catalog -> key ""
    if !config.catalog.is_empty() {
        let default_catalog = toml_map_to_string_map(&config.catalog);
        tracing::debug!(
            "Loaded default catalog with {} entries from .utoo.toml",
            default_catalog.len()
        );
        catalogs.insert(String::new(), default_catalog);
    }

    // Named catalogs
    for (name, entries) in &config.catalogs {
        let named = toml_map_to_string_map(entries);
        tracing::debug!(
            "Loaded catalog '{}' with {} entries from .utoo.toml",
            name,
            named.len()
        );
        catalogs.insert(name.clone(), named);
    }

    catalogs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_catalogs_default_and_named() {
        let dir = TempDir::new().unwrap();
        let toml_content = r#"
[catalog]
lodash = "^4.17.21"
react = "^18.0.0"
address = "2"

[catalogs.legacy]
path-to-regexp = "^1.9.0"
"#;
        fs::write(dir.path().join(".utoo.toml"), toml_content).unwrap();

        let catalogs = load_catalogs(dir.path());

        // Default catalog
        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("lodash"), Some(&"^4.17.21".to_string()));
        assert_eq!(default.get("react"), Some(&"^18.0.0".to_string()));
        assert_eq!(default.get("address"), Some(&"2".to_string()));

        // Named catalog
        let legacy = catalogs.get("legacy").unwrap();
        assert_eq!(legacy.get("path-to-regexp"), Some(&"^1.9.0".to_string()));
    }

    #[test]
    fn test_load_catalogs_no_file() {
        let dir = TempDir::new().unwrap();
        let catalogs = load_catalogs(dir.path());
        assert!(catalogs.is_empty());
    }

    #[test]
    fn test_load_catalogs_empty_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".utoo.toml"), "").unwrap();

        let catalogs = load_catalogs(dir.path());
        assert!(catalogs.is_empty());
    }

    #[test]
    fn test_load_catalogs_coexists_with_existing_config() {
        let dir = TempDir::new().unwrap();
        // .utoo.toml may already have key-value pairs for other settings
        let toml_content = r#"
registry = "https://registry.npmmirror.com"

[catalog]
lodash = "^4.17.21"
"#;
        fs::write(dir.path().join(".utoo.toml"), toml_content).unwrap();

        let catalogs = load_catalogs(dir.path());
        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("lodash"), Some(&"^4.17.21".to_string()));
    }

    #[test]
    fn test_load_catalogs_multiple_named() {
        let dir = TempDir::new().unwrap();
        let toml_content = r#"
[catalog]
react = "^18.0.0"

[catalogs.legacy]
path-to-regexp = "^1.9.0"

[catalogs.next]
react = "^19.0.0"
"#;
        fs::write(dir.path().join(".utoo.toml"), toml_content).unwrap();

        let catalogs = load_catalogs(dir.path());

        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("react"), Some(&"^18.0.0".to_string()));

        let legacy = catalogs.get("legacy").unwrap();
        assert_eq!(legacy.get("path-to-regexp"), Some(&"^1.9.0".to_string()));

        let next = catalogs.get("next").unwrap();
        assert_eq!(next.get("react"), Some(&"^19.0.0".to_string()));
    }
}
