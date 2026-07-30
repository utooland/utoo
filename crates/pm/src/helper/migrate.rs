use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::util::config_file::Config;
use crate::util::json::load_package_json;

#[derive(Debug)]
pub struct MigrateResult {
    pub fields: Vec<(String, usize)>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FromPm {
    #[default]
    None,
    Pnpm,
}

/// Field mapping from pnpm-workspace.yaml to package.json.
/// (pnpm_key, package_json_key)
const PNPM_PKG_FIELD_MAP: &[(&str, &str)] =
    &[("packages", "workspaces"), ("overrides", "overrides")];

/// TOML keys that pnpm migration is allowed to write into .utoo.toml.
/// Only these sections are merged; all other existing config is preserved.
const PNPM_MIGRATE_KEYS: &[&str] = &["catalog", "catalogs"];

/// Fields we care about from pnpm-workspace.yaml.
/// Missing fields become `None`; present fields (even empty) become `Some`.
#[derive(Debug, Deserialize, Serialize)]
struct PnpmWorkspace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    packages: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalog: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    catalogs: Option<HashMap<String, HashMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    overrides: Option<HashMap<String, String>>,
}

/// Migrate a pnpm project to utoo config files.
///
/// Reads `pnpm-workspace.yaml` and:
/// 1. Updates `package.json` with `workspaces` and `overrides`
/// 2. Writes `.utoo.toml` with catalog configuration
///
/// Must run before `Config::load` is first called so that `MERGED_CONFIG`
/// picks up the generated `.utoo.toml`.
pub async fn migrate_from_pnpm(root_path: &Path) -> Result<MigrateResult> {
    let yaml_path = root_path.join("pnpm-workspace.yaml");
    let yaml_content = match crate::fs::read_to_string(&yaml_path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("pnpm-workspace.yaml not found in {}", root_path.display());
        }
        Err(e) => return Err(anyhow::Error::from(e).context("Failed to read pnpm-workspace.yaml")),
    };

    let workspace: PnpmWorkspace =
        serde_yaml::from_str(&yaml_content).context("Failed to parse pnpm-workspace.yaml")?;

    // 1. Update package.json with mapped fields
    let ws_value = serde_json::to_value(&workspace)?;

    let pkg_updates: Vec<_> = PNPM_PKG_FIELD_MAP
        .iter()
        .filter_map(|(from, to)| ws_value.get(*from).map(|v| (*to, v.clone())))
        .collect();

    // Collect field counts from package.json updates
    let mut fields: Vec<(String, usize)> = pkg_updates
        .iter()
        .map(|(key, v)| {
            let len = v
                .as_array()
                .map_or_else(|| v.as_object().map_or(0, |o| o.len()), |a| a.len());
            (key.to_string(), len)
        })
        .filter(|(_, len)| *len > 0)
        .collect();

    if !pkg_updates.is_empty() {
        let mut pkg: serde_json::Value = load_package_json(root_path).await?;
        let pkg_obj = pkg
            .as_object_mut()
            .context("package.json is not an object")?;
        for (key, value) in pkg_updates {
            pkg_obj.insert(key.to_string(), value);
        }
        crate::fs::write(
            root_path.join("package.json"),
            serde_json::to_string_pretty(&pkg)? + "\n",
        )
        .await
        .context("Failed to write package.json")?;
    }

    // 2. Merge whitelisted keys into existing .utoo.toml
    let toml_updates: Vec<_> = PNPM_MIGRATE_KEYS
        .iter()
        .filter_map(|key| ws_value.get(*key).map(|v| (*key, v.clone())))
        .collect();

    if !toml_updates.is_empty() {
        let toml_path = root_path.join(".utoo.toml");
        let existing = Config::load_from_path(&toml_path).await?;
        let mut base = toml::Table::try_from(existing)?;
        let incoming: toml::Table = toml::from_str(&toml::to_string(&workspace)?)?;

        for key in PNPM_MIGRATE_KEYS {
            if let Some(value) = incoming.get(*key) {
                base.insert(key.to_string(), value.clone());
            }
        }

        crate::fs::write(&toml_path, toml::to_string_pretty(&base)?)
            .await
            .context("Failed to write .utoo.toml")?;

        // Count catalog entries
        let catalogs_count: usize = toml_updates
            .iter()
            .map(|(_, v)| {
                v.as_object().map_or(0, |o| {
                    // nested catalogs: sum inner map sizes; flat catalog: count keys
                    o.values()
                        .map(|iv| iv.as_object().map_or(1, |io| io.len()))
                        .sum()
                })
            })
            .sum();
        fields.push(("catalogs".to_string(), catalogs_count));
    }

    Ok(MigrateResult { fields })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::config_file::Config;

    #[test]
    fn test_parse_pnpm_workspace_yaml() {
        let yaml = r#"
packages:
  - packages/*
  - plugins/*

catalog:
  lodash: ^4.17.21
  typescript: ^5.0.0

catalogs:
  legacy:
    debug: ^3.2.7

overrides:
  vite: npm:rolldown-vite@^7.1.13
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        let packages = workspace.packages.unwrap();
        assert_eq!(packages, vec!["packages/*", "plugins/*"]);
        let catalog = workspace.catalog.unwrap();
        assert_eq!(catalog.get("lodash").unwrap(), "^4.17.21");
        let catalogs = workspace.catalogs.unwrap();
        assert_eq!(
            catalogs.get("legacy").unwrap().get("debug").unwrap(),
            "^3.2.7"
        );
        let overrides = workspace.overrides.unwrap();
        assert_eq!(overrides.get("vite").unwrap(), "npm:rolldown-vite@^7.1.13");
    }

    #[test]
    fn test_parse_empty_pnpm_workspace() {
        let yaml = "packages:\n  - packages/*\n";
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        assert!(workspace.packages.is_some());
        assert!(workspace.catalog.is_none());
        assert!(workspace.catalogs.is_none());
        assert!(workspace.overrides.is_none());
    }

    #[tokio::test]
    async fn test_migrate_missing_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        let result = migrate_from_pnpm(dir.path()).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("pnpm-workspace.yaml not found")
        );
    }

    #[tokio::test]
    async fn test_migrate_updates_package_json() {
        let dir = tempfile::tempdir().unwrap();

        let pkg = serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "catalog:"
            }
        });
        std::fs::write(
            dir.path().join("package.json"),
            serde_json::to_string_pretty(&pkg).unwrap(),
        )
        .unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n  - apps/*\noverrides:\n  vite: npm:rolldown-vite@^7\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();
        let ws = pkg["workspaces"].as_array().unwrap();
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0], "packages/*");
        assert_eq!(ws[1], "apps/*");
        assert_eq!(pkg["overrides"]["vite"], "npm:rolldown-vite@^7");
        assert_eq!(pkg["name"], "test-project");
        assert_eq!(pkg["dependencies"]["lodash"], "catalog:");
    }

    #[tokio::test]
    async fn test_migrate_writes_utoo_toml() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages: []\ncatalog:\n  lodash: ^4.17.21\n  debug: ^4.3.4\ncatalogs:\n  legacy:\n    debug: ^3.2.7\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let toml_content = std::fs::read_to_string(dir.path().join(".utoo.toml")).unwrap();
        assert!(toml_content.contains("lodash"));
        assert!(toml_content.contains("debug"));
        assert!(toml_content.contains("[catalogs.legacy]"));
    }

    #[test]
    fn test_parse_no_packages_field() {
        let yaml = r#"
catalog:
  react: ^18.0.0
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        assert!(workspace.packages.is_none());
        let catalog = workspace.catalog.unwrap();
        assert_eq!(catalog.get("react").unwrap(), "^18.0.0");
    }

    #[test]
    fn test_parse_scoped_packages_in_catalog() {
        let yaml = r#"
catalog:
  '@types/node': ^20.0.0
  '@eggjs/core': ^3.0.0
  lodash: ^4.17.21
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        let catalog = workspace.catalog.unwrap();
        assert_eq!(catalog.get("@types/node").unwrap(), "^20.0.0");
        assert_eq!(catalog.get("@eggjs/core").unwrap(), "^3.0.0");
        assert_eq!(catalog.get("lodash").unwrap(), "^4.17.21");
    }

    #[test]
    fn test_parse_npm_alias_in_overrides() {
        let yaml = r#"
overrides:
  vite: npm:rolldown-vite@^7.1.13
  typescript: ^5.0.0
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        let overrides = workspace.overrides.unwrap();
        assert_eq!(overrides.get("vite").unwrap(), "npm:rolldown-vite@^7.1.13");
        assert_eq!(overrides.get("typescript").unwrap(), "^5.0.0");
    }

    #[test]
    fn test_parse_multiple_named_catalogs() {
        let yaml = r#"
catalogs:
  react17:
    react: ^17.0.0
    react-dom: ^17.0.0
  react18:
    react: ^18.0.0
    react-dom: ^18.0.0
  legacy:
    debug: ^3.2.7
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        let catalogs = workspace.catalogs.unwrap();
        assert_eq!(catalogs.len(), 3);
        assert_eq!(catalogs["react17"].get("react").unwrap(), "^17.0.0");
        assert_eq!(catalogs["react18"].get("react").unwrap(), "^18.0.0");
        assert_eq!(catalogs["legacy"].get("debug").unwrap(), "^3.2.7");
    }

    #[test]
    fn test_parse_ignores_unknown_fields() {
        let yaml = r#"
packages:
  - packages/*
catalogMode: prefer
minimumReleaseAge: 1440
minimumReleaseAgeExclude:
  - '@eggjs/*'
  - vitest
catalog:
  lodash: ^4.17.21
"#;
        let workspace: PnpmWorkspace = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(workspace.packages.unwrap(), vec!["packages/*"]);
        assert_eq!(
            workspace.catalog.unwrap().get("lodash").unwrap(),
            "^4.17.21"
        );
    }

    #[tokio::test]
    async fn test_migrate_no_workspaces_only_catalogs() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-app", "dependencies": {"lodash": "catalog:"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "catalog:\n  lodash: ^4.17.21\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(pkg.get("workspaces").is_none());
        assert_eq!(pkg["name"], "my-app");
        assert_eq!(pkg["dependencies"]["lodash"], "catalog:");

        let toml_content = std::fs::read_to_string(dir.path().join(".utoo.toml")).unwrap();
        assert!(toml_content.contains("lodash"));
    }

    #[tokio::test]
    async fn test_migrate_no_catalogs_only_workspaces() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("package.json"), r#"{"name": "mono"}"#).unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n  - apps/*\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();
        let ws = pkg["workspaces"].as_array().unwrap();
        assert_eq!(ws.len(), 2);

        assert!(!dir.path().join(".utoo.toml").exists());
    }

    #[tokio::test]
    async fn test_migrate_preserves_existing_package_json_fields() {
        let dir = tempfile::tempdir().unwrap();

        let pkg = serde_json::json!({
            "name": "@eggjs/monorepo",
            "version": "4.0.0",
            "private": true,
            "type": "module",
            "scripts": {
                "build": "tsdown",
                "test": "vitest"
            },
            "devDependencies": {
                "typescript": "catalog:",
                "vitest": "catalog:"
            },
            "engines": {
                "node": ">=18"
            }
        });
        std::fs::write(
            dir.path().join("package.json"),
            serde_json::to_string_pretty(&pkg).unwrap(),
        )
        .unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\noverrides:\n  vite: npm:rolldown-vite@^7\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert!(pkg["workspaces"].is_array());
        assert_eq!(pkg["overrides"]["vite"], "npm:rolldown-vite@^7");

        assert_eq!(pkg["name"], "@eggjs/monorepo");
        assert_eq!(pkg["version"], "4.0.0");
        assert_eq!(pkg["private"], true);
        assert_eq!(pkg["type"], "module");
        assert_eq!(pkg["scripts"]["build"], "tsdown");
        assert_eq!(pkg["devDependencies"]["typescript"], "catalog:");
        assert_eq!(pkg["engines"]["node"], ">=18");
    }

    #[tokio::test]
    async fn test_migrate_preserves_existing_utoo_toml_config() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("package.json"), r#"{"name": "my-app"}"#).unwrap();

        // Pre-existing .utoo.toml with registry config
        std::fs::write(
            dir.path().join(".utoo.toml"),
            "[values]\nregistry = \"https://registry.npmmirror.com\"\n",
        )
        .unwrap();

        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "catalog:\n  lodash: ^4.17.21\ncatalogs:\n  legacy:\n    debug: ^3.2.7\n",
        )
        .unwrap();

        migrate_from_pnpm(dir.path()).await.unwrap();

        let config: Config =
            toml::from_str(&std::fs::read_to_string(dir.path().join(".utoo.toml")).unwrap())
                .unwrap();

        // Existing config preserved
        assert_eq!(
            config.get("registry").unwrap(),
            Some("https://registry.npmmirror.com".to_string())
        );

        // New catalogs added
        let catalogs = config.catalogs();
        assert_eq!(
            catalogs.get("").unwrap().get("lodash"),
            Some(&"^4.17.21".to_string())
        );
        assert_eq!(
            catalogs.get("legacy").unwrap().get("debug"),
            Some(&"^3.2.7".to_string())
        );
    }
}
