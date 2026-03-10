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

/// Model for the catalog-related sections of .utoo.toml.
///
/// Uses `#[serde(default)]` so missing sections are silently ignored,
/// and `#[serde(flatten)]` is NOT used -- we only extract the catalog
/// fields; all other .utoo.toml keys are ignored via deny_unknown_fields=false.
#[derive(Debug, Default, Deserialize)]
struct CatalogConfig {
    /// Default catalog: `[catalog]` section.
    #[serde(default)]
    catalog: HashMap<String, String>,

    /// Named catalogs: `[catalogs.<name>]` sections.
    #[serde(default)]
    catalogs: HashMap<String, HashMap<String, String>>,
}

/// Load catalog definitions from `.utoo.toml` in the given directory.
///
/// Returns an empty map if the file doesn't exist or contains no
/// catalog sections.  The default catalog is stored under key `""`
/// (empty string).
pub async fn load_catalogs(root_path: &Path) -> Catalogs {
    let toml_path = root_path.join(".utoo.toml");
    match crate::fs::read_to_string(&toml_path).await {
        Ok(content) => parse_catalogs(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(e) => {
            tracing::warn!("Failed to read {}: {}", toml_path.display(), e);
            HashMap::new()
        }
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

    let mut catalogs: Catalogs = config.catalogs;

    // Default catalog -> key ""
    if !config.catalog.is_empty() {
        catalogs.insert(String::new(), config.catalog);
    }

    if !catalogs.is_empty() {
        tracing::debug!("Loaded {} catalog(s) from .utoo.toml", catalogs.len());
    }

    catalogs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_load_catalogs_default_and_named() {
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

        let catalogs = load_catalogs(dir.path()).await;

        // Default catalog
        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("lodash"), Some(&"^4.17.21".to_string()));
        assert_eq!(default.get("react"), Some(&"^18.0.0".to_string()));
        assert_eq!(default.get("address"), Some(&"2".to_string()));

        // Named catalog
        let legacy = catalogs.get("legacy").unwrap();
        assert_eq!(legacy.get("path-to-regexp"), Some(&"^1.9.0".to_string()));
    }

    #[tokio::test]
    async fn test_load_catalogs_no_file() {
        let dir = TempDir::new().unwrap();
        let catalogs = load_catalogs(dir.path()).await;
        assert!(catalogs.is_empty());
    }

    #[tokio::test]
    async fn test_load_catalogs_empty_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".utoo.toml"), "").unwrap();

        let catalogs = load_catalogs(dir.path()).await;
        assert!(catalogs.is_empty());
    }

    #[tokio::test]
    async fn test_load_catalogs_coexists_with_existing_config() {
        let dir = TempDir::new().unwrap();
        // .utoo.toml may already have key-value pairs for other settings
        let toml_content = r#"
registry = "https://registry.npmmirror.com"

[catalog]
lodash = "^4.17.21"
"#;
        fs::write(dir.path().join(".utoo.toml"), toml_content).unwrap();

        let catalogs = load_catalogs(dir.path()).await;
        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("lodash"), Some(&"^4.17.21".to_string()));
    }

    #[tokio::test]
    async fn test_load_catalogs_multiple_named() {
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

        let catalogs = load_catalogs(dir.path()).await;

        let default = catalogs.get("").unwrap();
        assert_eq!(default.get("react"), Some(&"^18.0.0".to_string()));

        let legacy = catalogs.get("legacy").unwrap();
        assert_eq!(legacy.get("path-to-regexp"), Some(&"^1.9.0".to_string()));

        let next = catalogs.get("next").unwrap();
        assert_eq!(next.get("react"), Some(&"^19.0.0".to_string()));
    }
}
