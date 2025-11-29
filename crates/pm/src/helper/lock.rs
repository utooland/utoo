use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::helper::workspace::find_workspaces;
use crate::model::graph::DependencyGraph;
use crate::model::package_lock::LockPackage;
use crate::service::dependency_resolution::DependencyResolutionService;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::{load_package_json_from_path, load_package_lock_json_from_path};
use crate::util::registry::resolve;
use crate::util::relative_path::to_relative_path;
use crate::util::save_type::{PackageAction, SaveType};
use crate::util::{cache::parse_pattern, cloner::clone, downloader::download};

use super::workspace::find_workspace_path;

// Platform-specific line endings
#[cfg(target_os = "windows")]
const LINE_ENDING: &str = "\r\n";
#[cfg(not(target_os = "windows"))]
const LINE_ENDING: &str = "\n";

// Use the model's LockPackage but create a simplified PackageLock for helper functions
#[derive(Deserialize, Serialize, Clone)]
pub struct PackageLock {
    pub packages: HashMap<String, LockPackage>,
}

// Type alias for backward compatibility
pub type Package = LockPackage;

pub fn group_by_depth(
    packages: &HashMap<String, Package>,
) -> HashMap<usize, Vec<(String, Package)>> {
    let mut groups = HashMap::new();
    for (path, package) in packages {
        let depth = path.matches("node_modules").count();
        groups
            .entry(depth)
            .or_insert_with(Vec::new)
            .push((path.clone(), package.clone()));
    }
    groups
}

pub fn extract_package_name(path: &str) -> String {
    if let Some(index) = path.rfind("node_modules/") {
        let (_, package_path) = path.split_at(index + "node_modules/".len());
        package_path.to_string()
    } else {
        path.to_string()
    }
}

/// Normalize dependency field: convert empty objects to None for consistent comparison
fn normalize_deps_field(field: Option<&Value>) -> Option<&Value> {
    match field {
        Some(val) if val.as_object().is_some_and(|obj| obj.is_empty()) => None,
        other => other,
    }
}

/// Compare dependency fields, treating empty objects and None as equal
fn deps_fields_equal(pkg_field: Option<&Value>, lock_field: Option<&Value>) -> bool {
    normalize_deps_field(pkg_field) == normalize_deps_field(lock_field)
}

pub async fn ensure_package_lock(root_path: &Path) -> Result<PackageLock> {
    // Check package.json exists in project directory
    if tokio::fs::metadata(root_path.join("package.json"))
        .await
        .is_err()
    {
        return Err(anyhow!("package.json not found"));
    }

    // Check package-lock.json exists in project directory
    if tokio::fs::metadata(root_path.join("package-lock.json"))
        .await
        .is_err()
    {
        tracing::debug!("Resolving dependencies");
        // Build dependencies using service layer
        let package_lock = DependencyResolutionService::build_deps(root_path).await?;

        // Write to disk asynchronously in background
        let path = root_path.to_path_buf();
        let lock_clone = package_lock.clone();
        tokio::spawn(async move {
            let _ = save_package_lock(&path, &lock_clone).await;
        });

        Ok(package_lock)
    } else {
        // Validate dependencies to ensure package-lock.json is in sync with package.json
        if is_pkg_lock_outdated(root_path).await? {
            tracing::debug!("Resolving dependencies");
            // Build dependencies using service layer
            let package_lock = DependencyResolutionService::build_deps(root_path).await?;

            // Write to disk asynchronously in background
            let path = root_path.to_path_buf();
            let lock_clone = package_lock.clone();
            tokio::spawn(async move {
                let _ = save_package_lock(&path, &lock_clone).await;
            });

            return Ok(package_lock);
        }

        // Load existing package-lock.json only when it's valid and up-to-date
        tracing::debug!("Loading package-lock.json from current project for dependency download");
        let package_lock: PackageLock =
            crate::util::json::read_json_file(&root_path.join("package-lock.json")).await?;

        Ok(package_lock)
    }
}

/// Batch update package.json for multiple package specifications to reduce file I/O operations
pub async fn update_package_json(
    cwd: &Path,
    action: &PackageAction,
    specs: &[&str],
    workspace: &Option<String>,
    save_type: &SaveType,
) -> Result<()> {
    if specs.is_empty() {
        return Ok(());
    }

    // 1. Find target workspace if specified
    let target_dir = if let Some(ws) = workspace {
        find_workspace_path(cwd, ws)
            .await
            .context("Failed to find workspace path")?
    } else {
        cwd.to_path_buf()
    };

    // 2. Parse all package specs in parallel
    let mut package_specs = Vec::new();
    for spec in specs {
        let (name, version, version_spec) = parse_package_spec(spec).await?;
        package_specs.push((name, version, version_spec));
    }

    // 3. Read package.json once and detect trailing newline
    let package_json_path = target_dir.join("package.json");
    let package_json_content = tokio::fs::read_to_string(&package_json_path).await?;
    let has_trailing_newline =
        package_json_content.ends_with(LINE_ENDING) || package_json_content.ends_with('\n');
    let mut package_json: Value = serde_json::from_str(&package_json_content)?;

    let dep_field = match save_type {
        SaveType::Dev => "devDependencies",
        SaveType::Peer => "peerDependencies",
        SaveType::Optional => "optionalDependencies",
        SaveType::Prod => "dependencies",
    };

    // 4. Ensure dependencies field exists if we're adding packages
    if *action == PackageAction::Add && package_json.get(dep_field).is_none() {
        package_json[dep_field] = Value::Object(serde_json::Map::new());
    }

    // 5. Update all packages in memory
    if let Some(deps) = package_json.get_mut(dep_field)
        && let Some(deps_obj) = deps.as_object_mut()
    {
        for (name, version, version_spec) in package_specs {
            match action {
                PackageAction::Add => {
                    let version_to_write = match version_spec {
                        spec if spec.is_empty() || spec == "*" || spec == "latest" => {
                            format!("^{version}")
                        }
                        spec => spec.to_string(),
                    };
                    deps_obj.insert(name, Value::String(version_to_write));
                }
                PackageAction::Remove => {
                    deps_obj.remove(&name);
                }
            }
        }
    }

    // 6. Write back to package.json preserving trailing newline
    let mut content = serde_json::to_string_pretty(&package_json)?;
    if has_trailing_newline {
        content.push_str(LINE_ENDING);
    }
    tokio::fs::write(&package_json_path, content).await?;

    Ok(())
}

pub async fn parse_package_spec(spec: &str) -> Result<(String, String, String)> {
    let (name, version_spec) = parse_pattern(spec);
    let resolved = resolve(&name, &version_spec).await?;
    Ok((name, resolved.version, version_spec))
}

pub async fn prepare_global_package_json(npm_spec: &str, prefix: Option<&str>) -> Result<PathBuf> {
    // Parse package name and version
    let (name, _version, version_spec) = parse_package_spec(npm_spec).await?;
    let lib_path = match prefix {
        Some(prefix) => PathBuf::from(prefix).join("lib/node_modules"),
        None => {
            // Get current executable path
            let current_exe = std::env::current_exe()?;
            current_exe
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("lib/node_modules")
        }
    };

    tracing::debug!("lib_path: {}", lib_path.to_string_lossy());

    // Create global package directory
    let package_path = lib_path.join(&name);
    tokio::fs::create_dir_all(&package_path).await?;

    // Get package info from registry
    let resolved = resolve(&name, &version_spec).await?;

    // Get tarball URL from manifest
    let tarball_url = resolved.manifest["dist"]["tarball"]
        .as_str()
        .ok_or_else(|| anyhow!("Failed to get tarball URL from manifest"))?;

    // Download and extract package
    let cache_dir = crate::util::cache::get_cache_dir();
    let cache_path = cache_dir.join(format!("{}/{}", name, resolved.version));
    let cache_flag_path = cache_dir.join(format!("{}/{}/_resolved", name, resolved.version));

    // Download if not cached
    if !tokio::fs::try_exists(&cache_flag_path).await? {
        tracing::debug!("Downloading {} to {}", tarball_url, cache_path.display());
        download(tarball_url, &cache_path)
            .await
            .context("Failed to download package")?;

        // If the package has install scripts, create a flag file
        // in linux, we can use hardlink when FICLONE is not supported
        // so we need to copy the file to the package directory to avoid effect other packages
        if resolved.manifest.get("hasInstallScript") == Some(&json!(true)) {
            let has_install_script_flag_path = cache_path.join("_hasInstallScript");
            tokio::fs::write(has_install_script_flag_path, "").await?;
        }
    }

    // Clone to package directory
    tracing::debug!(
        "Cloning {} to {}",
        cache_path.display(),
        package_path.display()
    );
    clone(&cache_path, &package_path, true)
        .await
        .context("Failed to clone package")?;

    // Remove devDependencies from package.json
    let package_json_path = package_path.join("package.json");
    let package_json_content = tokio::fs::read_to_string(&package_json_path).await?;
    let mut package_json: Value = serde_json::from_str(&package_json_content)?;

    // Remove specified dependency fields and scripts.prepare
    let package_obj = package_json.as_object_mut().unwrap();
    package_obj.remove("devDependencies");

    // Remove scripts.prepare if it exists
    if let Some(scripts) = package_obj.get_mut("scripts")
        && let Some(scripts_obj) = scripts.as_object_mut()
    {
        scripts_obj.remove("prepare");
        scripts_obj.remove("prepublish");
    }

    // Write back the modified package.json
    tokio::fs::write(
        &package_json_path,
        serde_json::to_string_pretty(&package_json)?,
    )
    .await?;

    tracing::debug!("package_path: {}", package_path.to_string_lossy());
    Ok(package_path)
}

/// Extract the relative package name from a package directory path string.
/// Handles both normal and scoped packages, and skips invalid deep paths.
pub fn path_to_pkg_name(path_str: &str) -> Option<&str> {
    if let Some(idx) = path_str.rfind("node_modules/") {
        let pkg_name = &path_str[idx + "node_modules/".len()..];
        let parts: Vec<&str> = pkg_name.split('/').collect();
        // Only allow ora or @scope/ora, skip @pkg/name/path/custom/package.json
        if parts.len() > 2 || (parts.len() == 2 && !parts[0].starts_with('@')) {
            return None;
        }
        Some(pkg_name)
    } else {
        None
    }
}

pub async fn is_pkg_lock_outdated(root_path: &Path) -> Result<bool> {
    let pkg_file = load_package_json_from_path(root_path).await?;
    let lock_file = load_package_lock_json_from_path(root_path).await?;

    // get packages in package-lock.json
    let packages = lock_file
        .get("packages")
        .and_then(|p| p.as_object())
        .ok_or_else(|| anyhow!("Invalid package-lock.json format"))?;

    // prepare packages to check
    let mut pkgs_to_check = vec![("".to_string(), pkg_file.clone())];

    // populate all workspaces
    let workspaces = find_workspaces(root_path).await?;
    for (_, path, workspace_pkg) in workspaces {
        let target_path = to_relative_path(&path, root_path);
        pkgs_to_check.push((target_path, workspace_pkg));
    }

    // new workspace not found
    for (path, pkg) in pkgs_to_check {
        let lock = match packages.get(&path) {
            Some(lock) => lock,
            None => {
                let name = if path.is_empty() { "root" } else { &path };
                tracing::warn!("package-lock.json is outdated, new workspace {name} not found");
                return Ok(true);
            }
        };

        // check dependencies whether changed
        for (dep_field, _is_optional) in get_dep_types().await {
            if !deps_fields_equal(pkg.get(dep_field), lock.get(dep_field)) {
                let name = if path.is_empty() { "root" } else { &path };
                tracing::warn!("package-lock.json is outdated, {name} {dep_field} changed");
                return Ok(true);
            }
        }

        // only check engines for root workspace
        if path.is_empty() && pkg.get("engines") != lock.get("engines") {
            tracing::warn!("package-lock.json is outdated, engines changed");
            return Ok(true);
        }
    }

    Ok(false)
}

async fn get_dep_types() -> Vec<(&'static str, bool)> {
    let legacy_peer_deps = get_legacy_peer_deps().await;

    if legacy_peer_deps {
        vec![
            ("dependencies", false),
            ("optionalDependencies", true),
            ("devDependencies", false),
        ]
    } else {
        vec![
            ("dependencies", false),
            ("peerDependencies", false),
            ("optionalDependencies", true),
            ("devDependencies", false),
        ]
    }
}

/// Convert serialized packages Value to HashMap for PackageLock
fn convert_packages_to_hashmap(packages: Value) -> Result<HashMap<String, Package>> {
    serde_json::from_value(packages).with_context(|| "Failed to parse packages")
}

/// Build ideal tree and return PackageLock
pub async fn build_ideal_tree_to_package_lock(
    path: &Path,
    graph: &DependencyGraph,
) -> Result<PackageLock> {
    let (packages, total_packages) = graph.serialize_to_packages(path);

    tracing::debug!("Total {total_packages} dependencies after merging");

    // Convert packages Value to HashMap for PackageLock
    let packages_map = convert_packages_to_hashmap(packages)?;
    Ok(PackageLock {
        packages: packages_map,
    })
}

/// Save PackageLock to disk synchronously
pub async fn save_package_lock(path: &Path, package_lock: &PackageLock) -> Result<()> {
    // Get name and version from root package
    let (name, version) = package_lock
        .packages
        .get("")
        .map(|p| {
            (
                p.name.as_ref().unwrap_or(&String::new()).clone(),
                p.version.as_ref().unwrap_or(&String::new()).clone(),
            )
        })
        .unwrap_or_else(|| (String::new(), String::new()));

    let lock_file = json!({
        "name": name,
        "version": version,
        "lockfileVersion": 3,
        "requires": true,
        "packages": package_lock.packages,
    });

    let temp_path = path.join("package-lock.json.tmp");
    let target_path = path.join("package-lock.json");

    let content = serde_json::to_string_pretty(&lock_file)?;
    tokio::fs::write(&temp_path, content)
        .await
        .context("Failed to write temporary package-lock.json")?;
    tokio::fs::rename(temp_path, target_path)
        .await
        .context("Failed to rename package-lock.json")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_version_to_write() {
        // Test cases for different version specifications
        let test_cases = vec![
            ("1.2.3", "", "^1.2.3"),
            ("1.2.3", "*", "^1.2.3"),
            ("1.2.3", "latest", "^1.2.3"),
            ("1.2.3", "^1.2.0", "^1.2.0"),
            ("1.2.3", "~1.2.0", "~1.2.0"),
            ("1.2.3", "1.2.3", "1.2.3"),
        ];

        for (version, spec, expected) in test_cases {
            let version_to_write = match spec {
                spec if spec.is_empty() || spec == "*" || spec == "latest" => {
                    format!("^{version}")
                }
                spec => spec.to_string(),
            };
            assert_eq!(
                version_to_write, expected,
                "Failed for version: {version}, spec: {spec}",
            );
        }
    }

    #[test]
    fn test_path_to_pkg_name() {
        // Normal nested package
        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/a/node_modules/b"),
            Some("b")
        );
        // Top-level package
        assert_eq!(super::path_to_pkg_name("/root/node_modules/a"), Some("a"));

        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/@a/b"),
            Some("@a/b")
        );
        // Deep invalid path (should be None)
        assert_eq!(
            super::path_to_pkg_name("/root/node_modules/@a/b/node_modules/b/c/d"),
            None
        );
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Test case 1: package.json and package-lock.json are in sync
        let pkg_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        let pkg_lock = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0",
                    "dependencies": {
                        "lodash": "^4.17.20"
                    },
                    "devDependencies": {
                        "typescript": "^4.9.0"
                    }
                }
            }
        });

        // Write test files to temporary directory
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(temp_path.join("package-lock.json"), pkg_lock.to_string()).unwrap();

        // Test that files are in sync
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 2: package.json has new dependency
        let pkg_json_updated = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20",
                "react": "^18.0.0"  // New dependency
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        fs::write(temp_path.join("package.json"), pkg_json_updated.to_string()).unwrap();
        let outdated = is_pkg_lock_outdated(temp_path).await.unwrap();
        assert!(outdated);

        // Test case 3: package.json has updated version
        let pkg_json_version_updated = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.21"  // Updated version
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            }
        });

        fs::write(
            temp_path.join("package.json"),
            pkg_json_version_updated.to_string(),
        )
        .unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 4: package.json has removed dependency
        let pkg_json_removed = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            }
            // Removed devDependencies
        });

        fs::write(temp_path.join("package.json"), pkg_json_removed.to_string()).unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test case 4: package.json has removed dependency
        let pkg_json_engines_changed = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.20"
            },
            "devDependencies": {
                "typescript": "^4.9.0"
            },
            "engines": {
                "install-node": "16"
            }
            // Removed devDependencies
        });

        fs::write(
            temp_path.join("package.json"),
            pkg_json_engines_changed.to_string(),
        )
        .unwrap();
        assert!(is_pkg_lock_outdated(temp_path).await.unwrap());
    }

    #[test]
    fn test_package_struct_with_name_field() {
        // Test that Package struct can be deserialized with name field
        let package_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "resolved": "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz",
            "link": false
        });

        let package: Package = serde_json::from_value(package_json).unwrap();
        assert_eq!(package.name, Some("test-package".to_string()));
        assert_eq!(package.version, Some("1.0.0".to_string()));
        assert_eq!(
            package.resolved,
            Some("https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string())
        );
        assert_eq!(package.link, Some(false));
    }

    #[test]
    fn test_package_struct_without_name_field() {
        // Test that Package struct can be deserialized without name field (backward compatibility)
        let package_json = json!({
            "version": "1.0.0",
            "resolved": "https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz",
            "link": false
        });

        let package: Package = serde_json::from_value(package_json).unwrap();
        assert_eq!(package.name, None);
        assert_eq!(package.version, Some("1.0.0".to_string()));
        assert_eq!(
            package.resolved,
            Some("https://registry.npmjs.org/test-package/-/test-package-1.0.0.tgz".to_string())
        );
        assert_eq!(package.link, Some(false));
    }

    #[test]
    fn test_deps_fields_equal() {
        // Test case 1: Both None
        assert!(deps_fields_equal(None, None));

        // Test case 2: Both empty objects
        let empty_obj = json!({});
        assert!(deps_fields_equal(Some(&empty_obj), Some(&empty_obj)));

        // Test case 3: None vs empty object
        assert!(deps_fields_equal(None, Some(&empty_obj)));
        assert!(deps_fields_equal(Some(&empty_obj), None));

        // Test case 4: Both have same non-empty content
        let deps1 = json!({
            "lodash": "^4.17.20",
            "react": "^18.0.0"
        });
        let deps2 = json!({
            "lodash": "^4.17.20",
            "react": "^18.0.0"
        });
        assert!(deps_fields_equal(Some(&deps1), Some(&deps2)));

        // Test case 5: Different content
        let deps3 = json!({
            "lodash": "^4.17.20"
        });
        let deps4 = json!({
            "react": "^18.0.0"
        });
        assert!(!deps_fields_equal(Some(&deps3), Some(&deps4)));

        // Test case 6: Non-empty vs None
        let deps5 = json!({
            "lodash": "^4.17.20"
        });
        assert!(!deps_fields_equal(Some(&deps5), None));
        assert!(!deps_fields_equal(None, Some(&deps5)));

        // Test case 7: Non-empty vs empty object
        assert!(!deps_fields_equal(Some(&deps5), Some(&empty_obj)));
        assert!(!deps_fields_equal(Some(&empty_obj), Some(&deps5)));

        // Test case 8: Non-object values
        let string_val = json!("some-string");
        let number_val = json!(123);
        assert!(deps_fields_equal(Some(&string_val), Some(&string_val)));
        assert!(!deps_fields_equal(Some(&string_val), Some(&number_val)));
        assert!(!deps_fields_equal(Some(&string_val), None));
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_with_empty_deps() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Test case: package.json has empty dependencies object, package-lock.json has no dependencies field
        let pkg_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {}  // Empty object
        });

        let pkg_lock = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0"
                    // No dependencies field
                }
            }
        });

        // Write test files to temporary directory
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(temp_path.join("package-lock.json"), pkg_lock.to_string()).unwrap();

        // Test that empty object and missing field are treated as equal
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());

        // Test reverse case: package.json has no dependencies field, package-lock.json has empty dependencies
        let pkg_json_no_deps = json!({
            "name": "test-package",
            "version": "1.0.0"
            // No dependencies field
        });

        let pkg_lock_empty_deps = json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0",
                    "dependencies": {}  // Empty object
                }
            }
        });

        fs::write(temp_path.join("package.json"), pkg_json_no_deps.to_string()).unwrap();
        fs::write(
            temp_path.join("package-lock.json"),
            pkg_lock_empty_deps.to_string(),
        )
        .unwrap();

        // Test that missing field and empty object are treated as equal
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());
    }

    #[tokio::test]
    async fn test_update_package_json_preserves_trailing_newline() {
        use crate::util::save_type::{PackageAction, SaveType};
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        // Test case 1: package.json with trailing newline
        let pkg_json_with_newline = r#"{
  "name": "test-package",
  "version": "1.0.0",
  "dependencies": {}
}
"#;
        fs::write(temp_path.join("package.json"), pkg_json_with_newline).unwrap();

        update_package_json(
            temp_path,
            &PackageAction::Add,
            &["lodash@4.17.21"],
            &None,
            &SaveType::Prod,
        )
        .await
        .unwrap();

        let content = fs::read_to_string(temp_path.join("package.json")).unwrap();
        assert!(content.ends_with('\n'), "Should preserve trailing newline");

        // Test case 2: package.json without trailing newline
        let pkg_json_no_newline = r#"{
  "name": "test-package",
  "version": "1.0.0",
  "dependencies": {}
}"#;
        fs::write(temp_path.join("package.json"), pkg_json_no_newline).unwrap();

        update_package_json(
            temp_path,
            &PackageAction::Add,
            &["react@18.0.0"],
            &None,
            &SaveType::Prod,
        )
        .await
        .unwrap();

        let content = fs::read_to_string(temp_path.join("package.json")).unwrap();
        assert!(!content.ends_with('\n'), "Should not add trailing newline");
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_update_package_json_preserves_crlf() {
        use crate::util::save_type::{PackageAction, SaveType};
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        // package.json with CRLF line endings
        let pkg_json_crlf = "{\r\n  \"name\": \"test-package\",\r\n  \"version\": \"1.0.0\",\r\n  \"dependencies\": {}\r\n}\r\n";
        fs::write(temp_path.join("package.json"), pkg_json_crlf).unwrap();

        update_package_json(
            temp_path,
            &PackageAction::Add,
            &["lodash@4.17.21"],
            &None,
            &SaveType::Prod,
        )
        .await
        .unwrap();

        let content = fs::read_to_string(temp_path.join("package.json")).unwrap();
        assert!(
            content.ends_with("\r\n"),
            "Should preserve CRLF trailing newline on Windows"
        );
    }
}
