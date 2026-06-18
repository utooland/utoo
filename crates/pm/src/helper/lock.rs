use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, Result, anyhow};
use serde_json::Value;
use utoo_ruborist::builder::PeerDeps;
use utoo_ruborist::lock::{LockPackage, PackageLock};
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::registry::resolve_package;
use utoo_ruborist::runtime::install_runtime_from_map;
use utoo_ruborist::spec::{PackageSpec, Protocol, resolve_catalog_spec};

use super::ruborist_context::Context;
use super::workspace::find_workspace_path;
use crate::fs;
use crate::util::cli_enum::{PackageAction, SaveType};
use crate::util::git_resolver::{resolve_git_spec, resolve_github_spec};
use crate::util::json::{load_package_lock_json_from_path, read_json_file};
use crate::util::user_config::{
    get_catalogs, get_or_load_package_json, get_peer_deps, set_package_json,
};

// Platform-specific line endings
#[cfg(target_os = "windows")]
const LINE_ENDING: &str = "\r\n";
#[cfg(not(target_os = "windows"))]
const LINE_ENDING: &str = "\n";

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

pub async fn ensure_package_lock(root_path: &Path) -> Result<PackageLock> {
    // Check package.json exists in project directory
    if fs::metadata(root_path.join("package.json")).await.is_err() {
        return Err(anyhow!("package.json not found"));
    }

    // Check if we need to regenerate package-lock.json
    let needs_regenerate = fs::metadata(root_path.join("package-lock.json"))
        .await
        .is_err()
        || is_pkg_lock_outdated(root_path).await?;

    if needs_regenerate {
        tracing::debug!("Resolving dependencies");
        let lock = Context::build_deps(root_path.to_path_buf()).await?;

        // Write to disk asynchronously in background
        let path = root_path.to_path_buf();
        let lock_clone = lock.clone();
        tokio::spawn(async move {
            if let Err(e) = save_package_lock(&path, &lock_clone).await {
                tracing::warn!("Failed to save package-lock.json: {e}");
            }
        });

        return Ok(lock);
    }

    // Load existing package-lock.json only when it's valid and up-to-date
    tracing::debug!("Loading package-lock.json from current project");
    let package_lock: PackageLock = load_package_lock_json_from_path(root_path).await?;

    Ok(package_lock)
}

/// Batch update package.json for multiple package specifications to reduce file I/O operations
pub struct UpdatePackageJsonOptions<'a> {
    pub cwd: &'a Path,
    pub action: PackageAction,
    pub specs: &'a [&'a str],
    pub workspace: Option<&'a str>,
    pub save_type: SaveType,
}

pub async fn update_package_json(opts: &UpdatePackageJsonOptions<'_>) -> Result<()> {
    if opts.specs.is_empty() {
        return Ok(());
    }

    // 1. Find target workspace if specified
    let target_dir = if let Some(ws) = opts.workspace {
        find_workspace_path(opts.cwd, ws)
            .await
            .context("Failed to find workspace path")?
    } else {
        opts.cwd.to_path_buf()
    };

    // 2. Parse all package specs in parallel
    let mut package_specs = Vec::new();
    for spec in opts.specs {
        let (name, version, version_spec) = resolve_package_spec(spec).await?;
        package_specs.push((name, version, version_spec));
    }

    // 3. Read package.json once and detect trailing newline
    let package_json_path = target_dir.join("package.json");
    let package_json_content = fs::read_to_string(&package_json_path).await?;
    let has_trailing_newline =
        package_json_content.ends_with(LINE_ENDING) || package_json_content.ends_with('\n');
    let mut package_json: Value = serde_json::from_str(&package_json_content)?;

    let dep_field = match opts.save_type {
        SaveType::Dev => "devDependencies",
        SaveType::Peer => "peerDependencies",
        SaveType::Optional => "optionalDependencies",
        SaveType::Prod => "dependencies",
    };

    // 4. Ensure dependencies field exists if we're adding packages
    if opts.action == PackageAction::Add && package_json.get(dep_field).is_none() {
        package_json[dep_field] = Value::Object(serde_json::Map::new());
    }

    // 5. Update all packages in memory
    if let Some(deps) = package_json.get_mut(dep_field)
        && let Some(deps_obj) = deps.as_object_mut()
    {
        for (name, version, version_spec) in package_specs {
            match opts.action {
                PackageAction::Add => {
                    deps_obj.insert(
                        name,
                        Value::String(format_save_spec(&version_spec, &version)),
                    );
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
    fs::write(&package_json_path, content).await?;

    // Write-through: update the package.json cache
    if let Ok(updated_pkg) = serde_json::from_value::<PackageJson>(package_json) {
        set_package_json(&target_dir, updated_pkg);
    }

    Ok(())
}

/// Format a version spec for writing into package.json.
///
/// Git/non-registry specs are written as-is (e.g. the resolved URL with pinned commit).
/// Wildcard specs (`*`, `latest`, empty) are pinned to `^<resolved_version>`.
/// Everything else (semver ranges, exact versions) passes through unchanged.
pub fn format_save_spec(version_spec: &str, resolved_version: &str) -> String {
    // Non-registry specs (git, github, file, http, etc.) are written as-is.
    if version_spec.parse::<Protocol>().is_ok() {
        return version_spec.to_string();
    }
    match version_spec {
        "" | "*" | "latest" => format!("^{resolved_version}"),
        _ => version_spec.to_string(),
    }
}

pub async fn resolve_package_spec(spec: &str) -> Result<(String, String, String)> {
    let parsed = PackageSpec::from(spec);
    match parsed {
        PackageSpec::Registry { name, version_spec } => {
            let resolved = resolve_package(&Context::registry().await, &name, &version_spec)
                .await
                .context("Failed to resolve package")?;
            Ok((name, resolved.version, version_spec))
        }
        PackageSpec::Git { url, commit_ish } => {
            let resolved = resolve_git_spec(&url, commit_ish.as_deref(), None).await?;
            Ok((
                resolved.name.clone(),
                resolved.version.clone(),
                resolved.resolved_url.clone(),
            ))
        }
        PackageSpec::GitHub {
            owner,
            repo,
            commit_ish,
        } => {
            let resolved = resolve_github_spec(&owner, &repo, commit_ish.as_deref()).await?;
            Ok((
                resolved.name.clone(),
                resolved.version.clone(),
                resolved.resolved_url.clone(),
            ))
        }
        PackageSpec::Local { protocol, .. } => {
            anyhow::bail!("Local spec ({protocol}:) not supported in this context")
        }
        PackageSpec::Http { url } => {
            anyhow::bail!("HTTP tarball spec ({url}) not supported in this context")
        }
    }
}

/// Root-entry optionalDependencies as the lock will have them: user's own
/// merged with the synthetic `node-bin-*` deps injected from
/// `engines.install-node`. Returns `None` when no merge is needed and the
/// caller should compare against `pkg.optional_dependencies` directly.
fn root_optional_with_runtime(path: &str, pkg: &PackageJson) -> Option<HashMap<String, String>> {
    if !path.is_empty() {
        return None;
    }
    let runtime = install_runtime_from_map(pkg.engines.as_ref()?);
    if runtime.is_empty() {
        return None;
    }
    let mut merged = pkg.optional_dependencies.clone().unwrap_or_default();
    for (name, version) in runtime {
        merged.entry(name).or_insert(version);
    }
    Some(merged)
}

pub async fn is_pkg_lock_outdated(root_path: &Path) -> Result<bool> {
    // Root package.json is served from the in-process cache. The cache
    // stays consistent with disk across the full `ut install` lifetime
    // because `update_package_json` calls `set_package_json` write-through
    // after every edit, and a fresh `ut install` process starts with a
    // cold cache (so external edits / git checkout always see a re-read).
    // The lockfile itself is still read from disk each time — it's the
    // thing whose freshness we're checking.
    let root_pkg = get_or_load_package_json(root_path).await?;
    let lock_file: PackageLock = read_json_file(&root_path.join("package-lock.json")).await?;

    let catalogs = get_catalogs().await;
    let deps_match = |pkg_deps: Option<&HashMap<String, String>>,
                      lock_deps: Option<&HashMap<String, String>>|
     -> bool {
        let empty = HashMap::new();
        let pkg_deps = pkg_deps.unwrap_or(&empty);
        match lock_deps {
            None => pkg_deps.is_empty(),
            Some(ld) => {
                pkg_deps.len() == ld.len()
                    && pkg_deps.iter().all(|(name, spec)| {
                        let resolved = resolve_catalog_spec(name, spec, &catalogs).unwrap_or(spec);
                        ld.get(name).is_some_and(|v| v == resolved)
                    })
            }
        }
    };

    let packages = &lock_file.packages;
    let peer_deps = get_peer_deps().await;

    let workspaces = Context::discovery().find_workspaces(root_path).await?;
    let mut pkgs_to_check: Vec<(String, &PackageJson)> = Vec::with_capacity(1 + workspaces.len());
    pkgs_to_check.push((String::new(), &root_pkg));
    for ws in &workspaces {
        let target_path = ws
            .path
            .strip_prefix(root_path)
            .unwrap_or(&ws.path)
            .to_string_lossy()
            .to_string();
        pkgs_to_check.push((target_path, &ws.package_json));
    }

    for (path, pkg) in &pkgs_to_check {
        let lock = match packages.get(path.as_str()) {
            Some(lock) => lock,
            None => {
                let name = if path.is_empty() { "root" } else { path };
                tracing::warn!("package-lock.json is outdated, new workspace {name} not found");
                return Ok(true);
            }
        };

        let name = if path.is_empty() { "root" } else { path };

        if !deps_match(pkg.dependencies.as_ref(), lock.dependencies.as_ref()) {
            tracing::warn!("package-lock.json is outdated, {name} dependencies changed");
            return Ok(true);
        }

        // `Context::build_deps` folds synthetic `node-bin-*` runtime deps into
        // the root lock entry from `engines.install-node`; mirror that here so
        // a project using `install-node` isn't judged outdated forever.
        let runtime_merged = root_optional_with_runtime(path, pkg);
        let expected_optional = runtime_merged
            .as_ref()
            .or(pkg.optional_dependencies.as_ref());

        if !deps_match(expected_optional, lock.optional_dependencies.as_ref()) {
            tracing::warn!("package-lock.json is outdated, {name} optionalDependencies changed");
            return Ok(true);
        }

        if !deps_match(
            pkg.dev_dependencies.as_ref(),
            lock.dev_dependencies.as_ref(),
        ) {
            tracing::warn!("package-lock.json is outdated, {name} devDependencies changed");
            return Ok(true);
        }

        if peer_deps == PeerDeps::Include
            && !deps_match(
                pkg.peer_dependencies.as_ref(),
                lock.peer_dependencies.as_ref(),
            )
        {
            tracing::warn!("package-lock.json is outdated, {name} peerDependencies changed");
            return Ok(true);
        }
    }

    // engines: root only — derived from the same loaded `root_pkg`
    let root_lock = packages
        .get("")
        .ok_or_else(|| anyhow!("Missing root in package-lock.json"))?;
    let pkg_engines = root_pkg.engines.as_ref().filter(|m| !m.is_empty());
    let lock_engines = root_lock.engines.as_ref().filter(|m| !m.is_empty());

    let engines_match = match (pkg_engines, lock_engines) {
        (None, None) => true,
        (Some(p), Some(l)) => *p == *l,
        _ => false,
    };

    if !engines_match {
        tracing::warn!("package-lock.json is outdated, engines changed");
        return Ok(true);
    }

    Ok(false)
}

/// Save PackageLock to disk synchronously
pub async fn save_package_lock(path: &Path, package_lock: &PackageLock) -> Result<()> {
    let temp_path = path.join("package-lock.json.tmp");
    let target_path = path.join("package-lock.json");

    // PackageLock now has all required fields (name, version, lockfile_version, requires, packages)
    let content = serde_json::to_string_pretty(package_lock)?;
    fs::write(&temp_path, content)
        .await
        .context("Failed to write temporary package-lock.json")?;
    fs::rename(temp_path, target_path)
        .await
        .context("Failed to rename package-lock.json")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::util::cli_enum::{PackageAction, SaveType};

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
    fn test_format_save_spec() {
        // Wildcard / empty specs pin to ^resolved
        assert_eq!(format_save_spec("", "1.2.3"), "^1.2.3");
        assert_eq!(format_save_spec("*", "1.2.3"), "^1.2.3");
        assert_eq!(format_save_spec("latest", "1.2.3"), "^1.2.3");

        // Normal semver specs pass through
        assert_eq!(format_save_spec("^1.0.0", "1.2.3"), "^1.0.0");
        assert_eq!(format_save_spec("~1.2.0", "1.2.3"), "~1.2.0");
        assert_eq!(format_save_spec("1.2.3", "1.2.3"), "1.2.3");

        // Non-registry specs pass through as-is
        assert_eq!(
            format_save_spec("git+https://github.com/user/repo.git#abc123", "1.0.0"),
            "git+https://github.com/user/repo.git#abc123"
        );
        assert_eq!(
            format_save_spec("git://github.com/user/repo.git", "1.0.0"),
            "git://github.com/user/repo.git"
        );
        assert_eq!(
            format_save_spec("github:user/repo", "1.0.0"),
            "github:user/repo"
        );
        assert_eq!(
            format_save_spec("https://example.com/pkg.tgz", "1.0.0"),
            "https://example.com/pkg.tgz"
        );
        assert_eq!(
            format_save_spec("file:../local-pkg", "1.0.0"),
            "file:../local-pkg"
        );
    }

    /// Baseline lockfile for the outdated-check scenarios.
    ///
    /// Each scenario uses a fresh `TempDir` (unique path) so the in-process
    /// package.json cache never serves a stale entry — which mirrors real
    /// `ut install` where every invocation starts with a cold cache.
    fn baseline_pkg_lock() -> Value {
        json!({
            "name": "test-package",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "test-package",
                    "version": "1.0.0",
                    "dependencies": { "lodash": "^4.17.20" },
                    "devDependencies": { "typescript": "^4.9.0" }
                }
            }
        })
    }

    async fn assert_outdated(pkg_json: Value, expected: bool) {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(
            temp_path.join("package-lock.json"),
            baseline_pkg_lock().to_string(),
        )
        .unwrap();
        assert_eq!(
            is_pkg_lock_outdated(temp_path).await.unwrap(),
            expected,
            "package.json:\n{pkg_json}"
        );
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_in_sync() {
        assert_outdated(
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": { "lodash": "^4.17.20" },
                "devDependencies": { "typescript": "^4.9.0" }
            }),
            false,
        )
        .await;
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_new_dep_added() {
        assert_outdated(
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "lodash": "^4.17.20",
                    "react": "^18.0.0"
                },
                "devDependencies": { "typescript": "^4.9.0" }
            }),
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_version_bumped() {
        assert_outdated(
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": { "lodash": "^4.17.21" },
                "devDependencies": { "typescript": "^4.9.0" }
            }),
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_dev_deps_removed() {
        assert_outdated(
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": { "lodash": "^4.17.20" }
            }),
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_install_node_in_sync() {
        // package.json never carries the synthetic node-bin optionalDependencies
        // that `Context::build_deps` injects from `engines.install-node`, but
        // the lock does — the check must still treat this pair as in-sync.
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let pkg_json = json!({
            "name": "test-package",
            "version": "1.0.0",
            "engines": { "install-node": "16" }
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
                    "optionalDependencies": {
                        "node-darwin-x64": "16",
                        "node-bin-darwin-arm64": "16",
                        "node-linux-x64": "16",
                        "node-linux-arm64": "16",
                        "node-win-x64": "16",
                        "node-win-x86": "16"
                    },
                    "engines": { "install-node": "16" }
                }
            }
        });
        fs::write(temp_path.join("package.json"), pkg_json.to_string()).unwrap();
        fs::write(temp_path.join("package-lock.json"), pkg_lock.to_string()).unwrap();
        assert!(!is_pkg_lock_outdated(temp_path).await.unwrap());
    }

    #[tokio::test]
    async fn test_is_pkg_lock_outdated_engines_added() {
        assert_outdated(
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": { "lodash": "^4.17.20" },
                "devDependencies": { "typescript": "^4.9.0" },
                "engines": { "install-node": "16" }
            }),
            true,
        )
        .await;
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

        update_package_json(&UpdatePackageJsonOptions {
            cwd: temp_path,
            action: PackageAction::Add,
            specs: &["lodash@4.17.21"],
            workspace: None,
            save_type: SaveType::Prod,
        })
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

        update_package_json(&UpdatePackageJsonOptions {
            cwd: temp_path,
            action: PackageAction::Add,
            specs: &["react@18.0.0"],
            workspace: None,
            save_type: SaveType::Prod,
        })
        .await
        .unwrap();

        let content = fs::read_to_string(temp_path.join("package.json")).unwrap();
        assert!(!content.ends_with('\n'), "Should not add trailing newline");
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_update_package_json_preserves_crlf() {
        let temp_dir = tempdir().unwrap();
        let temp_path = temp_dir.path();

        // package.json with CRLF line endings
        let pkg_json_crlf = "{\r\n  \"name\": \"test-package\",\r\n  \"version\": \"1.0.0\",\r\n  \"dependencies\": {}\r\n}\r\n";
        fs::write(temp_path.join("package.json"), pkg_json_crlf).unwrap();

        update_package_json(&UpdatePackageJsonOptions {
            cwd: temp_path,
            action: PackageAction::Add,
            specs: &["lodash@4.17.21"],
            workspace: None,
            save_type: SaveType::Prod,
        })
        .await
        .unwrap();

        let content = fs::read_to_string(temp_path.join("package.json")).unwrap();
        assert!(
            content.ends_with("\r\n"),
            "Should preserve CRLF trailing newline on Windows"
        );
    }
}
