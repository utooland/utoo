//! Normalize a package's manifest for publishing/packing.
//!
//! npm consumers cannot understand the `workspace:` and `catalog:` dependency
//! protocols — installing a tarball whose `package.json` still contains them
//! fails with `EUNSUPPORTEDPROTOCOL`. pnpm/bun rewrite these specifiers to
//! concrete semver ranges when they pack or publish; utoo must do the same.
//!
//! [`normalize_publish_manifest`] rewrites:
//!
//! - `workspace:*` / `workspace:^` / `workspace:~` / `workspace:<range>` →
//!   the linked workspace package's version (see [`resolve_workspace_spec`])
//! - `catalog:` / `catalog:<name>` → the version pinned in the catalog
//!   (`pnpm-workspace.yaml` → `.utoo.toml`, see [`resolve_catalog_spec`])
//! - pnpm's allowlisted `publishConfig` manifest overrides (including
//!   `main`, `types`, `bin`, and `exports`) → their top-level fields
//!
//! It is a no-op (returns `None`) when the manifest contains neither dependency
//! protocol nor a supported publish-time override, so ordinary packages keep
//! their on-disk `package.json` byte-for-byte.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::spec::{Catalogs, Protocol, resolve_catalog_spec, resolve_workspace_spec};

use crate::helper::ruborist_context::Context;
use crate::util::config_file::Config;

/// Return a normalized clone of `pkg` with publish-time overrides applied and
/// `workspace:`/`catalog:` dependency specifiers rewritten to concrete version
/// ranges, or `Ok(None)` when there is nothing to rewrite.
///
/// `package_root` is the directory of the package being packed; it is used to
/// locate the surrounding workspace when resolving `workspace:` specifiers.
pub(crate) async fn normalize_publish_manifest(
    package_root: &Path,
    pkg: &PackageJson,
) -> Result<Option<PackageJson>> {
    let needs_workspace = has_protocol(pkg, Protocol::Workspace);
    let needs_catalog = has_protocol(pkg, Protocol::Catalog);
    let needs_publish_override = has_publish_config_overrides(pkg);
    if !needs_workspace && !needs_catalog && !needs_publish_override {
        return Ok(None);
    }

    let (workspace_versions, catalogs) = if needs_workspace || needs_catalog {
        // Both protocols resolve against the surrounding workspace root:
        // sibling members supply `workspace:` versions, and the root
        // `.utoo.toml` holds catalog definitions. `pm-pack` runs inside a
        // member dir, so look up the root rather than assuming the cwd.
        let root = Context::discovery()
            .find_root_path(package_root)
            .await
            .with_context(|| {
                format!(
                    "failed to locate the workspace root for {}",
                    package_root.display()
                )
            })?;
        let workspace_versions = if needs_workspace {
            discover_workspace_versions(&root).await?
        } else {
            HashMap::new()
        };
        let catalogs = if needs_catalog {
            load_catalogs(&root).await?
        } else {
            Catalogs::new()
        };
        (workspace_versions, catalogs)
    } else {
        (HashMap::new(), Catalogs::new())
    };

    let mut normalized = pkg.clone();
    rewrite_dep_specs(&mut normalized, &workspace_versions, &catalogs)?;
    apply_publish_config_overrides(&mut normalized)?;
    Ok(Some(normalized))
}

/// pnpm's allowlist of package manifest fields that may be replaced at pack
/// time. Operational settings such as `registry`, `tag`, `access`, and
/// `provenance` intentionally remain nested under `publishConfig`.
const PUBLISH_CONFIG_OVERRIDE_FIELDS: &[&str] = &[
    "name",
    "bin",
    "engines",
    "type",
    "imports",
    "main",
    "module",
    "typings",
    "types",
    "exports",
    "browser",
    "esnext",
    "es2015",
    "unpkg",
    "umd:main",
    "os",
    "cpu",
    "libc",
    "typesVersions",
];

/// Whether any pnpm-compatible publish-time manifest override is present,
/// including an explicit JSON `null` value.
fn has_publish_config_overrides(pkg: &PackageJson) -> bool {
    pkg.publish_config.as_ref().is_some_and(|config| {
        PUBLISH_CONFIG_OVERRIDE_FIELDS
            .iter()
            .any(|field| config.extra.contains_key(*field))
    })
}

/// Apply pnpm-compatible `publishConfig` overrides to the packed manifest
/// without changing the source manifest on disk. Consumed keys are removed,
/// and an empty `publishConfig` object is omitted from package.json.
fn apply_publish_config_overrides(pkg: &mut PackageJson) -> Result<bool> {
    if !has_publish_config_overrides(pkg) {
        return Ok(false);
    }

    let mut manifest = pkg.to_value();
    let root = manifest
        .as_object_mut()
        .context("package manifest must serialize to a JSON object")?;
    let Some(serde_json::Value::Object(mut publish_config)) = root.remove("publishConfig") else {
        return Ok(false);
    };

    let mut applied = false;
    for field in PUBLISH_CONFIG_OVERRIDE_FIELDS {
        if let Some(value) = publish_config.remove(*field) {
            root.insert((*field).to_string(), value);
            applied = true;
        }
    }
    if !publish_config.is_empty() {
        root.insert(
            "publishConfig".to_string(),
            serde_json::Value::Object(publish_config),
        );
    }

    *pkg = PackageJson::from_value(&manifest)
        .context("publishConfig contains an invalid package manifest override")?;
    Ok(applied)
}

/// Rewrite every `workspace:`/`catalog:` specifier across all dependency maps
/// in place. Bails with a descriptive error if a specifier cannot be resolved
/// — leaving it in place would only defer the failure to the consumer's
/// `npm install`.
fn rewrite_dep_specs(
    pkg: &mut PackageJson,
    workspace_versions: &HashMap<String, String>,
    catalogs: &Catalogs,
) -> Result<()> {
    let maps = [
        &mut pkg.dependencies,
        &mut pkg.dev_dependencies,
        &mut pkg.peer_dependencies,
        &mut pkg.optional_dependencies,
    ];
    for (name, spec) in maps.into_iter().flatten().flat_map(|m| m.iter_mut()) {
        rewrite_spec(name, spec, workspace_versions, catalogs)?;
    }
    Ok(())
}

/// Rewrite a single dependency `spec` in place if it uses the `workspace:` or
/// `catalog:` protocol; leave any other spec untouched.
fn rewrite_spec(
    name: &str,
    spec: &mut String,
    workspace_versions: &HashMap<String, String>,
    catalogs: &Catalogs,
) -> Result<()> {
    match Protocol::strip_prefix(spec) {
        Some((Protocol::Workspace, _)) => {
            let version = workspace_versions.get(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot resolve workspace dependency `{name}` (\"{spec}\"): \
                     no workspace package named `{name}` with a version was found"
                )
            })?;
            *spec = resolve_workspace_spec(spec, version).expect("spec has workspace: prefix");
        }
        Some((Protocol::Catalog, _)) => {
            let resolved = resolve_catalog_spec(name, spec, catalogs).ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot resolve catalog dependency `{name}` (\"{spec}\"): \
                     no matching catalog entry (check `pnpm-workspace.yaml` / `.utoo.toml`)"
                )
            })?;
            *spec = resolved.to_string();
        }
        _ => {}
    }
    Ok(())
}

/// Build a `name -> version` map of every package in the workspace rooted at
/// `root`, used to resolve `workspace:` specifiers.
async fn discover_workspace_versions(root: &Path) -> Result<HashMap<String, String>> {
    let members = Context::discovery()
        .find_workspaces(root)
        .await
        .with_context(|| {
            format!(
                "failed to enumerate workspace packages under {}",
                root.display()
            )
        })?;

    if members.is_empty() {
        bail!(
            "manifest uses `workspace:` dependencies but no workspace packages \
             were discovered from root {}",
            root.display(),
        );
    }

    Ok(members
        .into_iter()
        .filter(|m| !m.package_json.version.is_empty())
        .map(|m| (m.name, m.package_json.version))
        .collect())
}

/// Load catalog definitions from the workspace root's `.utoo.toml` (written by
/// `ut install --from pnpm` from `pnpm-workspace.yaml`).
///
/// `Config::load_from_path` already treats a missing file as an empty config,
/// so only a malformed or unreadable `.utoo.toml` surfaces an error here — and
/// that should fail loudly rather than silently yield an empty catalog (which
/// would later read as a confusing "no matching catalog entry").
async fn load_catalogs(root: &Path) -> Result<Catalogs> {
    Ok(Config::load_from_path(&root.join(".utoo.toml"))
        .await
        .context("failed to load catalog config from .utoo.toml")?
        .catalogs())
}

/// Whether any dependency map contains a spec using `protocol`.
fn has_protocol(pkg: &PackageJson, protocol: Protocol) -> bool {
    [
        &pkg.dependencies,
        &pkg.dev_dependencies,
        &pkg.peer_dependencies,
        &pkg.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    .flat_map(|m| m.values())
    .any(|spec| Protocol::strip_prefix(spec).is_some_and(|(p, _)| p == protocol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pkg_with_deps(deps: &[(&str, &str)]) -> PackageJson {
        let map: HashMap<String, String> = deps
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        PackageJson {
            name: "host".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Some(map),
            ..Default::default()
        }
    }

    fn workspace_versions(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn catalogs(pairs: &[(&str, &str)]) -> Catalogs {
        let default: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Catalogs::from([(String::new(), default)])
    }

    #[test]
    fn rewrites_workspace_and_catalog_specs() {
        let mut pkg = pkg_with_deps(&[
            ("@scope/star", "workspace:*"),
            ("@scope/caret", "workspace:^"),
            ("@scope/range", "workspace:^2.0.0"),
            ("lodash", "catalog:"),
            ("react", "^18.0.0"),
        ]);
        let ws = workspace_versions(&[
            ("@scope/star", "1.5.0"),
            ("@scope/caret", "3.1.0"),
            ("@scope/range", "2.4.0"),
        ]);
        let cat = catalogs(&[("lodash", "^4.17.21")]);

        rewrite_dep_specs(&mut pkg, &ws, &cat).unwrap();

        let deps = pkg.dependencies.unwrap();
        assert_eq!(deps["@scope/star"], "1.5.0");
        assert_eq!(deps["@scope/caret"], "^3.1.0");
        assert_eq!(deps["@scope/range"], "^2.0.0");
        assert_eq!(deps["lodash"], "^4.17.21");
        // Untouched: a normal registry range.
        assert_eq!(deps["react"], "^18.0.0");
    }

    #[test]
    fn rewrites_across_all_dependency_kinds() {
        let mut pkg = PackageJson {
            name: "host".to_string(),
            version: "1.0.0".to_string(),
            dev_dependencies: Some(HashMap::from([(
                "dev-pkg".to_string(),
                "workspace:~".to_string(),
            )])),
            peer_dependencies: Some(HashMap::from([(
                "peer-pkg".to_string(),
                "workspace:*".to_string(),
            )])),
            optional_dependencies: Some(HashMap::from([(
                "opt-pkg".to_string(),
                "catalog:legacy".to_string(),
            )])),
            ..Default::default()
        };
        let ws = workspace_versions(&[("dev-pkg", "0.2.0"), ("peer-pkg", "5.0.0")]);
        let mut cat = Catalogs::new();
        cat.insert(
            "legacy".to_string(),
            HashMap::from([("opt-pkg".to_string(), "^1.0.0".to_string())]),
        );

        rewrite_dep_specs(&mut pkg, &ws, &cat).unwrap();

        assert_eq!(pkg.dev_dependencies.unwrap()["dev-pkg"], "~0.2.0");
        assert_eq!(pkg.peer_dependencies.unwrap()["peer-pkg"], "5.0.0");
        assert_eq!(pkg.optional_dependencies.unwrap()["opt-pkg"], "^1.0.0");
    }

    #[test]
    fn errors_on_unresolvable_workspace_dep() {
        let mut pkg = pkg_with_deps(&[("missing", "workspace:*")]);
        let err = rewrite_dep_specs(&mut pkg, &HashMap::new(), &Catalogs::new()).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn errors_on_unresolvable_catalog_dep() {
        let mut pkg = pkg_with_deps(&[("missing", "catalog:")]);
        let err = rewrite_dep_specs(&mut pkg, &HashMap::new(), &Catalogs::new()).unwrap_err();
        assert!(err.to_string().contains("catalog"));
    }

    #[test]
    fn has_protocol_detects_protocols() {
        let ws = pkg_with_deps(&[("a", "workspace:*")]);
        assert!(has_protocol(&ws, Protocol::Workspace));
        assert!(!has_protocol(&ws, Protocol::Catalog));

        let cat = pkg_with_deps(&[("a", "catalog:")]);
        assert!(has_protocol(&cat, Protocol::Catalog));
        assert!(!has_protocol(&cat, Protocol::Workspace));

        let plain = pkg_with_deps(&[("a", "^1.0.0")]);
        assert!(!has_protocol(&plain, Protocol::Workspace));
        assert!(!has_protocol(&plain, Protocol::Catalog));
    }

    #[tokio::test]
    async fn applies_publish_config_overrides_without_mutating_source_manifest() {
        let pkg = PackageJson::from_value(&json!({
            "name": "fixture",
            "version": "1.0.0",
            "main": "./src/index.ts",
            "types": "./src/index.ts",
            "bin": {
                "fixture": "./src/cli.ts"
            },
            "exports": {
                ".": "./src/index.ts",
                "./feature": "./src/feature.ts"
            },
            "publishConfig": {
                "main": "./dist/index.js",
                "types": "./dist/index.d.ts",
                "bin": {
                    "fixture": "./dist/cli.js"
                },
                "exports": {
                    ".": {
                        "import": "./dist/index.js",
                        "types": "./dist/index.d.ts"
                    },
                    "./feature": "./dist/feature.js"
                }
            }
        }))
        .unwrap();
        let source = pkg.to_value();

        let normalized = normalize_publish_manifest(Path::new("."), &pkg)
            .await
            .unwrap()
            .unwrap();
        let published = normalized.to_value();

        assert_eq!(source["main"], "./src/index.ts");
        assert_eq!(source["exports"]["."], "./src/index.ts");
        assert_eq!(
            source["publishConfig"]["exports"]["./feature"],
            "./dist/feature.js"
        );
        assert_eq!(published["main"], "./dist/index.js");
        assert_eq!(published["types"], "./dist/index.d.ts");
        assert_eq!(published["bin"]["fixture"], "./dist/cli.js");
        assert_eq!(published["exports"]["."]["import"], "./dist/index.js");
        assert_eq!(published["exports"]["."]["types"], "./dist/index.d.ts");
        assert_eq!(published["exports"]["./feature"], "./dist/feature.js");
        assert!(published.get("publishConfig").is_none());
    }

    #[test]
    fn applies_full_pnpm_allowlist_and_keeps_publish_options() {
        let overrides = json!({
            "name": "fixture-published",
            "bin": { "fixture": "./dist/cli.js" },
            "engines": { "node": ">=22" },
            "type": "module",
            "imports": { "#internal": "./dist/internal.js" },
            "main": "./dist/index.js",
            "module": "./dist/index.js",
            "typings": "./dist/index.d.ts",
            "types": "./dist/index.d.ts",
            "exports": { ".": "./dist/index.js" },
            "browser": "./dist/browser.js",
            "esnext": "./dist/esnext.js",
            "es2015": "./dist/es2015.js",
            "unpkg": "./dist/index.min.js",
            "umd:main": "./dist/index.umd.js",
            "os": [ "darwin" ],
            "cpu": [ "arm64" ],
            "libc": [ "glibc" ],
            "typesVersions": { "*": { "*": [ "dist/*" ] } }
        });
        let mut publish_config = overrides.as_object().unwrap().clone();
        publish_config.insert(
            "registry".to_string(),
            json!("https://registry.example.com"),
        );
        publish_config.insert("customSetting".to_string(), json!(true));
        let mut pkg = PackageJson::from_value(&json!({
            "name": "fixture",
            "version": "1.0.0",
            "publishConfig": publish_config
        }))
        .unwrap();

        assert!(apply_publish_config_overrides(&mut pkg).unwrap());
        let published = pkg.to_value();
        for (field, expected) in overrides.as_object().unwrap() {
            assert_eq!(&published[field], expected, "override field {field}");
            assert!(published["publishConfig"].get(field).is_none());
        }
        assert_eq!(
            published["publishConfig"]["registry"],
            "https://registry.example.com"
        );
        assert_eq!(published["publishConfig"]["customSetting"], true);
    }
}
