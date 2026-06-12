use crate::helper::ruborist_context::Context as FsContext;
use crate::model::package::{LifecycleHook, LifecycleScripts, PackageInfo};
use crate::util::cli_enum::ScriptPolicy;
use crate::util::logger::{PROGRESS_BAR, finish_progress_bar, log_progress, start_progress_bar};
use anyhow::{Context, Result};
use futures;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::lock::{LockPackage, PackageLock};
use utoo_ruborist::manifest::{ScriptsView, parse_bin_field};

use super::script::{LifecycleSink, MissingScript, ScriptOutput, ScriptService};
use super::workspace::{ResolvedWorkspaces, WorkspaceFilter, WorkspaceService};

/// npm install lifecycle ordered by event chain, each expanding to
/// `pre<event>` / `<event>` / `post<event>` via [`ScriptService::run_lifecycle`].
const NPM_INSTALL_EVENTS: &[&str] = &["install", "prepublish", "prepare"];

/// True if `path` contains `node_modules` as a distinct path segment — not just
/// as a substring of some other segment (e.g. a workspace dir literally named
/// `node_modules-utils`). Handles both `/` and `\` separators.
fn has_node_modules_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|seg| seg == "node_modules")
}

/// Whether `pkg` is a workspace's `node_modules` link — a link whose `resolved`
/// target is one of the workspace source dirs. A `file:<dir>` dep is also a link
/// but resolves to a non-workspace dir, so it returns `false` and keeps its own
/// scripts. `resolved` is normalized to `/` to match the normalized source keys.
fn links_to_workspace_source(pkg: &LockPackage, workspace_sources: &HashSet<String>) -> bool {
    pkg.is_link()
        && pkg
            .resolved
            .as_deref()
            .is_some_and(|target| workspace_sources.contains(&target.replace('\\', "/")))
}

/// Execution queues for package scripts and binary linking
/// Each entry is (PackageInfo, is_optional) where is_optional indicates if the package
/// is an optional dependency (based on edge type in dependency graph)
#[derive(Default)]
pub struct ExecutionQueues {
    pub preinstall: Vec<(Rc<PackageInfo>, bool)>,
    pub bin_linking: Vec<(Rc<PackageInfo>, bool)>,
    pub install: Vec<(Rc<PackageInfo>, bool)>,
    pub postinstall: Vec<(Rc<PackageInfo>, bool)>,
}

pub struct PackageService;

impl PackageService {
    pub async fn process_project_hooks(root_path: &Path) -> Result<()> {
        let package_info = PackageInfo::load(root_path).await?;
        Self::run_install_lifecycle(&package_info, None).await
    }

    /// Walk workspaces in topological order so a downstream workspace can
    /// consume build artifacts produced by an upstream workspace's `prepare`
    /// (npm 7+ semantics, fixes #2833).
    pub async fn process_workspace_install_hooks(root_path: &Path) -> Result<()> {
        let layers = match WorkspaceService::resolve_layers(root_path, WorkspaceFilter::All).await?
        {
            ResolvedWorkspaces::Layers { layers, .. } => layers,
            ResolvedWorkspaces::Current => return Ok(()),
        };

        let mut by_name: HashMap<String, PackageInfo> = FsContext::discovery()
            .find_workspaces(root_path)
            .await?
            .into_iter()
            .map(|ws| {
                let info = PackageInfo::from_package_json(&ws.path, &ws.package_json)
                    .with_context(|| format!("Failed to load workspace {}", ws.name))?;
                Ok((ws.name, info))
            })
            .collect::<Result<_>>()?;

        for layer in layers {
            for name in layer {
                let package = by_name.remove(&name).with_context(|| {
                    format!("workspace {name} present in topology but missing from workspace map")
                })?;
                Self::run_install_lifecycle(&package, Some(&name)).await?;
            }
        }

        Ok(())
    }

    async fn run_install_lifecycle(
        package: &PackageInfo,
        workspace_label: Option<&str>,
    ) -> Result<()> {
        for &event in NPM_INSTALL_EVENTS {
            ScriptService::run_lifecycle(
                package,
                event,
                &[],
                LifecycleSink::Stream { workspace_label },
                MissingScript::Skip,
            )
            .await
            .with_context(|| match workspace_label {
                Some(label) => format!("Failed to execute {event} lifecycle for {label}"),
                None => format!("Failed to execute project {event} lifecycle"),
            })?;
        }
        Ok(())
    }

    async fn read_lifecycle_scripts(package_path: &Path) -> Result<LifecycleScripts> {
        let s: ScriptsView = crate::util::json::load_package_json(package_path).await?;
        Ok(LifecycleScripts::from_scripts(&s.scripts))
    }

    /// Collect packages from memory PackageLock object with early filtering
    /// Returns Vec<(PackageInfo, is_optional)> where is_optional is determined by the edge type
    pub async fn collect_packages_from_lock(
        package_lock: &PackageLock,
        root_path: &Path,
        scripts: ScriptPolicy,
    ) -> Result<Vec<(PackageInfo, bool)>> {
        tracing::debug!("Collecting packages from memory lock...");

        // This collector is utoo's equivalent of npm's `depNodes` rebuild set
        // (`arborist/lib/arborist/rebuild.js` `#retrieveNodesByType`). npm keeps
        // a workspace's install lifecycle out of that set with two operations,
        // both reproduced below:
        //   1. it splits `node.isLink` nodes out (links run in a separate build);
        //   2. it then `depNodes.delete(node.target)` — drops each link's target
        //      (the workspace source dir), citing npm/cli#2905 "lifecycle scripts
        //      twice".
        // utoo's "links build" is `process_workspace_install_hooks` (the
        // topological workspace walk), which — unlike npm — covers only declared
        // `workspaces`, NOT `file:<dir>` deps. So a workspace link is excluded
        // here, but a `file:` link (no workspace-walk coverage) keeps running its
        // scripts in this collector.
        //
        // Workspace source dirs are the only non-root lock entries keyed without
        // a `node_modules` segment (deps/links all live under `node_modules/`);
        // a workspace link is then any `link:true` entry whose `resolved` points
        // back at one of those sources.
        let workspace_source_paths: HashSet<String> = package_lock
            .packages
            .keys()
            .filter(|p| !p.is_empty() && !has_node_modules_segment(p))
            .map(|p| p.replace('\\', "/"))
            .collect();

        let mut packages = Vec::new();
        for (path, lock_package) in &package_lock.packages {
            if path.is_empty() {
                continue;
            }

            // (2) Drop the workspace source dir (npm's `depNodes.delete(target)`).
            // Its lifecycle runs via the workspace walk, and npm never bin-links
            // the source dir — collecting it here re-runs its scripts on top of
            // the walk.
            if !has_node_modules_segment(path) {
                continue;
            }

            // (1) Split out link nodes (npm's `node.isLink` branch). The
            // serializer no longer stamps script markers on links (see #3097), so
            // `has_scripts` is always false for them and their scripts are decided
            // from disk instead. A *workspace* link has its scripts suppressed
            // (owned by the workspace walk) and is kept only for bin linking — how
            // a workspace `bin` lands in `node_modules/.bin` (npm's `#linkAllBins`);
            // a `file:` link keeps running its own scripts here.
            let is_link = lock_package.is_link();
            let is_workspace_link =
                links_to_workspace_source(lock_package, &workspace_source_paths);

            // Early filtering based on scripts parameter
            let has_scripts = lock_package.has_install_scripts();
            let package_name = lock_package.get_name(path);
            let bin_files = lock_package
                .bin
                .as_ref()
                .map(|bin| parse_bin_field(bin, &package_name))
                .unwrap_or_default();
            let has_bin = !bin_files.is_empty();

            // A workspace link contributes bin linking only — its install
            // scripts run via `process_workspace_install_hooks`. With no bin it
            // has nothing left to do here, so drop it.
            if is_workspace_link && !has_bin {
                continue;
            }

            // Skip packages that don't meet the filter criteria. A link node is
            // never dropped here on the `has_scripts` test (it carries no script
            // marker in the lock); its scripts are read from disk below.
            if scripts == ScriptPolicy::Ignore && !has_bin {
                continue; // scripts mode: only process packages with binaries
            }
            if scripts == ScriptPolicy::Run && !has_scripts && !has_bin && !is_link {
                continue; // full mode: process packages with scripts or binaries
            }

            // Check platform compatibility
            let is_compatible = lock_package.cpu.as_ref().is_none_or(is_cpu_compatible)
                && lock_package.os.as_ref().is_none_or(is_os_compatible);

            if !is_compatible {
                tracing::debug!("Package {path} is not compatible with current platform");
                continue;
            }

            let package_path = PathBuf::from(format!("{}/{}", root_path.display(), path));

            // Skip if package directory doesn't exist (e.g., omitted by --production/--omit)
            if !package_path.exists() {
                tracing::debug!("Package {path} not installed, skipping rebuild");
                continue;
            }

            // Read lifecycle scripts from package.json only if needed. A
            // workspace link contributes bin linking only — leave its scripts
            // empty so `create_execution_queues` never queues them (owned by
            // `process_workspace_install_hooks`).
            let lifecycle_scripts =
                if !is_workspace_link && (has_scripts || scripts == ScriptPolicy::Run) {
                    match Self::read_lifecycle_scripts(&package_path).await {
                        Ok(s) => s,
                        // A `file:` link can resolve to a missing or degenerate
                        // `package.json` (e.g. a conflict artifact keyed
                        // `node_modules/` with an empty name). Treat that as "no
                        // scripts" rather than failing the whole install; a real
                        // dependency with an unreadable manifest still errors.
                        Err(_) if is_link => LifecycleScripts::default(),
                        Err(e) => {
                            return Err(e).with_context(|| {
                                format!("Failed to read scripts for package: {path}")
                            });
                        }
                    }
                } else {
                    LifecycleScripts::default()
                };

            // Check if this package is an optional dependency (based on edge type)
            let is_optional = lock_package.is_optional();

            let package_info = PackageInfo {
                path: package_path,
                bin_files,
                scripts: Default::default(),
                lifecycle_scripts,
                name: package_name,
            };

            packages.push((package_info, is_optional));
        }
        Ok(packages)
    }

    /// Create execution queues with bins_only parameter support
    /// Takes Vec<(PackageInfo, is_optional)> where is_optional indicates edge type
    pub fn create_execution_queues_with_options(
        packages: Vec<(PackageInfo, bool)>,
        scripts: ScriptPolicy,
    ) -> Result<ExecutionQueues> {
        tracing::debug!("Creating execution queues with options...");
        let mut queues = ExecutionQueues::default();

        for (package, is_optional) in packages {
            let package = Rc::new(package);

            // Script queues - skip in bins_only mode
            if scripts == ScriptPolicy::Run {
                for (hook, queue) in [
                    (LifecycleHook::Preinstall, &mut queues.preinstall),
                    (LifecycleHook::Install, &mut queues.install),
                    (LifecycleHook::Postinstall, &mut queues.postinstall),
                ] {
                    if package.lifecycle_scripts.get_script(hook).is_some() {
                        queue.push((Rc::clone(&package), is_optional));
                    }
                }
            }

            // Binary linking queue - always process if package has bin files
            if !package.bin_files.is_empty() {
                tracing::debug!("Adding {} to bin linking queue", package.path.display());
                queues.bin_linking.push((Rc::clone(&package), is_optional));
            }
        }

        tracing::debug!(
            "Queue creation completed, {} tasks pending",
            queues.preinstall.len()
                + queues.bin_linking.len()
                + queues.install.len()
                + queues.postinstall.len()
        );

        Ok(queues)
    }

    /// Execute queues with bins_only parameter support
    pub async fn execute_queues_with_options(
        queues: ExecutionQueues,
        scripts: ScriptPolicy,
    ) -> Result<()> {
        if scripts == ScriptPolicy::Ignore {
            // Binary-only mode: only execute binary linking
            Self::execute_binary_linking(&queues.bin_linking).await?;
        } else {
            // Full mode: execute all queues in sequence
            let total_scripts =
                queues.preinstall.len() + queues.install.len() + queues.postinstall.len();
            let scripts_start = std::time::Instant::now();
            if total_scripts > 0 {
                start_progress_bar();
                PROGRESS_BAR.set_length(total_scripts as u64);
            }

            // Execute preinstall scripts in parallel
            Self::execute_script_queue(&queues.preinstall, LifecycleHook::Preinstall).await?;

            Self::execute_binary_linking(&queues.bin_linking).await?;

            Self::execute_script_queue(&queues.install, LifecycleHook::Install).await?;

            Self::execute_script_queue(&queues.postinstall, LifecycleHook::Postinstall).await?;

            if total_scripts > 0 {
                finish_progress_bar("scripts executed", Some(scripts_start.elapsed()));
            }
        }
        Ok(())
    }

    /// Execute script queue for a specific script type
    /// Queue contains (PackageInfo, is_optional) tuples where is_optional indicates edge type
    async fn execute_script_queue(
        queue: &[(Rc<PackageInfo>, bool)],
        hook: LifecycleHook,
    ) -> Result<()> {
        let queue_start = std::time::Instant::now();
        tracing::debug!("Starting {} queue with {} scripts", hook, queue.len());

        let script_tasks: Vec<_> = queue
            .iter()
            .filter_map(|(package, is_optional)| {
                let script = package.lifecycle_scripts.get_script(hook)?;
                Some({
                    let package = Rc::clone(package);
                    let script = script.to_string();
                    let is_optional = *is_optional;
                    async move {
                        log_progress(&format!("{} {}", package.name, hook));
                        let start = std::time::Instant::now();
                        let result =
                            ScriptService::execute_script(&package, hook, ScriptOutput::Silent)
                                .await
                                .with_context(|| {
                                    format!(
                                        "Failed to execute {} script for {} (command: {})",
                                        hook, package.name, script
                                    )
                                });
                        let elapsed = start.elapsed();
                        tracing::debug!(
                            "[{:.2}s] {} {} completed (path: {}, script: {})",
                            elapsed.as_secs_f64(),
                            package.name,
                            hook,
                            package.path.display(),
                            script
                        );
                        PROGRESS_BAR.inc(1);
                        (is_optional, result)
                    }
                })
            })
            .collect();

        // Wait for all script tasks to complete
        let script_results: Vec<(bool, Result<()>)> = futures::future::join_all(script_tasks).await;
        for (is_optional, result) in script_results {
            if let Err(e) = result {
                if is_optional {
                    // `{:#}` prints the full cause chain — this warn is the only
                    // signal the user gets that an optional dep's script failed.
                    tracing::warn!("Optional dependency script failed (ignored): {e:#}");
                } else {
                    return Err(e);
                }
            }
        }

        let queue_elapsed = queue_start.elapsed();
        tracing::debug!(
            "{} queue completed in {:.2}s",
            hook,
            queue_elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Execute binary file linking for packages.
    ///
    /// Queue contains (PackageInfo, is_optional) tuples - is_optional is not used
    /// here as binary linking happens only for successfully installed packages.
    ///
    /// Strategy:
    ///   1. Sort packages by path so module-local bin-name collisions resolve
    ///      deterministically (shallower / lex-smaller path wins).
    ///   2. Dedupe by `link_path` in a `HashSet` — first writer wins, mirroring
    ///      npm `bin-links/lib/link-gently.js`'s seen-set. Without this,
    ///      multiple packages declaring the same bin (e.g. `svgo` + `svgo-browser`)
    ///      both call into `link_bin`, which resolves via "remove existing,
    ///      symlink new" and yields non-deterministic last-writer-wins.
    ///   3. Walk the deduped jobs serially calling sync `link_bin`. Each
    ///      `link_bin` is `try_exists` + `symlink_metadata` + `symlink(2)` —
    ///      microsecond-class syscalls where async `tokio::fs::*` overhead
    ///      (spawn_blocking + mpsc + thread hop) dominates real cost. Bench
    ///      data on ant-design (228 bins) showed 36ms async → 6ms sync; adding
    ///      try_join_all/rayon parallelism only saved an additional 2-3ms over
    ///      sync, well within stddev — not worth the concurrency complexity.
    async fn execute_binary_linking(queue: &[(Rc<PackageInfo>, bool)]) -> Result<()> {
        // Sort by package path for deterministic dedupe winners across runs
        // (`collect_packages_from_lock` walks a HashMap, so input order is
        // non-deterministic without this).
        let mut ordered: Vec<&(Rc<PackageInfo>, bool)> = queue.iter().collect();
        ordered.sort_by(|a, b| a.0.path.cmp(&b.0.path));

        let mut seen: HashSet<PathBuf> = HashSet::new();

        for (package, _is_optional) in ordered {
            if package.bin_files.is_empty() {
                continue;
            }
            // Hoist out of the inner loop: `get_bin_dir` walks `path.ancestors()`
            // looking for `node_modules`, but the result is constant per package.
            let bin_dir = package
                .get_bin_dir()
                .with_context(|| format!("Failed to get bin directory for {}", package.name))?;
            for (bin_name, relative_path) in &package.bin_files {
                let link_path = bin_dir.join(bin_name);
                if !seen.insert(link_path.clone()) {
                    continue;
                }

                let target_path = package.path.join(relative_path);
                if !crate::fs::try_exists(&target_path).await? {
                    tracing::debug!(
                        "Binary file {} does not exist, skipping",
                        target_path.display()
                    );
                    continue;
                }

                ScriptService::ensure_executable(&target_path)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to ensure binary is executable for {} (path: {})",
                            package.name,
                            target_path.display()
                        )
                    })?;

                crate::util::linker::link_bin(&target_path, &link_path).with_context(|| {
                    format!(
                        "Failed to link binary for {} (from: {} to: {})",
                        package.name,
                        target_path.display(),
                        link_path.display()
                    )
                })?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_process_project_hooks_basic() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with basic project hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "echo 'Running preinstall hook'",
                "postinstall": "echo 'Running postinstall hook'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_ok(), "process_project_hooks should succeed");
    }

    #[tokio::test]
    async fn test_process_project_hooks_no_scripts() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json without scripts
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0"
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks - should succeed even without scripts
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "process_project_hooks should succeed even without scripts"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_with_scoped_package() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with scoped package name
        let package_json = json!({
            "name": "@scope/test-project",
            "version": "1.0.0",
            "scripts": {
                "prepare": "echo 'Running prepare hook for scoped package'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test process_project_hooks with scoped package
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "process_project_hooks should work with scoped packages"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_all_supported_hooks() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with all supported hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "echo 'preinstall'",
                "install": "echo 'install'",
                "postinstall": "echo 'postinstall'",
                "prepublish": "echo 'prepublish'",
                "preprepare": "echo 'preprepare'",
                "prepare": "echo 'prepare'",
                "postprepare": "echo 'postprepare'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that all hooks are executed
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "All supported hooks should be executed successfully"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_working_directory() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create a subdirectory structure
        let sub_dir = project_path.join("subproject");
        fs::create_dir_all(&sub_dir).unwrap();

        // Create package.json in subdirectory with script that checks working directory
        let package_json = json!({
            "name": "test-subproject",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "pwd | grep subproject"
            }
        });

        fs::write(
            sub_dir.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that scripts run in the correct directory (root_path)
        let result = PackageService::process_project_hooks(&sub_dir).await;
        assert!(
            result.is_ok(),
            "Scripts should run in the correct working directory based on root_path"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_npm_package_json_env() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with script that checks npm_package_json environment variable
        let expected_package_json_path = project_path.join("package.json");
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", expected_package_json_path.display())
            }
        });

        fs::write(
            &expected_package_json_path,
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that npm_package_json environment variable points to the correct path
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_ok(),
            "npm_package_json environment variable should point to the correct package.json path"
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_script_failure() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with failing script
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "preinstall": "exit 1"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that script failure is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(
            result.is_err(),
            "Script failure should be properly propagated"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to execute project")
        );
    }

    #[tokio::test]
    async fn test_process_project_hooks_invalid_package_json() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create invalid package.json
        fs::write(project_path.join("package.json"), "invalid json content").unwrap();

        // Test that invalid package.json is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_err(), "Invalid package.json should cause error");
    }

    #[tokio::test]
    async fn test_process_project_hooks_missing_package_json() {
        // Create temporary directory without package.json
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Test that missing package.json is properly handled
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_err(), "Missing package.json should cause error");
    }

    #[tokio::test]
    async fn test_process_project_hooks_partial_scripts() {
        // Create temporary directory
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path();

        // Create package.json with only some hooks
        let package_json = json!({
            "name": "test-project",
            "version": "1.0.0",
            "scripts": {
                "install": "echo 'install only'",
                "prepare": "echo 'prepare only'"
            }
        });

        fs::write(
            project_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Test that only existing hooks are executed
        let result = PackageService::process_project_hooks(project_path).await;
        assert!(result.is_ok(), "Only existing hooks should be executed");
    }

    #[tokio::test]
    async fn test_process_project_hooks_different_root_paths() {
        // Create multiple project directories to test path isolation
        let temp_dir = TempDir::new().unwrap();
        let project1_path = temp_dir.path().join("project1");
        let project2_path = temp_dir.path().join("project2");

        fs::create_dir_all(&project1_path).unwrap();
        fs::create_dir_all(&project2_path).unwrap();

        // Create different package.json files
        let package_json1 = json!({
            "name": "project1",
            "version": "1.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", project1_path.join("package.json").display())
            }
        });

        let package_json2 = json!({
            "name": "project2",
            "version": "2.0.0",
            "scripts": {
                "preinstall": format!("test \"$npm_package_json\" = \"{}\"", project2_path.join("package.json").display())
            }
        });

        fs::write(
            project1_path.join("package.json"),
            serde_json::to_string_pretty(&package_json1).unwrap(),
        )
        .unwrap();

        fs::write(
            project2_path.join("package.json"),
            serde_json::to_string_pretty(&package_json2).unwrap(),
        )
        .unwrap();

        // Test that each project gets the correct environment variables
        let result1 = PackageService::process_project_hooks(&project1_path).await;
        assert!(
            result1.is_ok(),
            "Project1 hooks should succeed with correct environment"
        );

        let result2 = PackageService::process_project_hooks(&project2_path).await;
        assert!(
            result2.is_ok(),
            "Project2 hooks should succeed with correct environment"
        );
    }

    #[tokio::test]
    async fn test_execute_queues_skips_missing_bin_file() {
        // Create a temporary directory for the fake package
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        // Create a package.json with a bin entry pointing to a non-existent file
        let package_json = serde_json::json!({
            "name": "test-bin-missing",
            "version": "1.0.0",
            "bin": {
                "testbin": "not-exist.js"
            }
        });
        fs::write(
            package_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Construct PackageInfo manually
        let package_info = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: vec![("testbin".to_string(), "not-exist.js".to_string())],
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::default(),
            name: "test-bin-missing".to_string(),
        };

        // Prepare queues: only bin linking queue has this package
        // The bool indicates is_optional (false = not optional)
        let queues = ExecutionQueues {
            bin_linking: vec![(Rc::new(package_info), false)],
            ..Default::default()
        };

        // Should not panic or error, even though the bin file does not exist
        let result = PackageService::execute_queues_with_options(queues, ScriptPolicy::Run).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_collect_packages_from_lock_with_scripts() {
        let temp_dir = TempDir::new().unwrap();

        // Create test packages in memory
        let mut packages = HashMap::new();

        // Package with both scripts and binaries
        packages.insert(
            "node_modules/full-package".to_string(),
            LockPackage {
                name: Some("full-package".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"cli": "bin/cli.js"})),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Package with only binaries
        packages.insert(
            "node_modules/bin-only".to_string(),
            LockPackage {
                name: Some("bin-only".to_string()),
                version: Some("2.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "index.js"})),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        // Package with only scripts
        packages.insert(
            "node_modules/script-only".to_string(),
            LockPackage {
                name: Some("script-only".to_string()),
                version: Some("3.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Package with neither scripts nor binaries
        packages.insert(
            "node_modules/no-hooks".to_string(),
            LockPackage {
                name: Some("no-hooks".to_string()),
                version: Some("4.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        let package_lock =
            PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages);

        // Create minimal package.json files for testing
        let node_modules = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();

        for package_name in &["full-package", "bin-only", "script-only", "no-hooks"] {
            let package_dir = node_modules.join(package_name);
            std::fs::create_dir_all(&package_dir).unwrap();

            let package_json = json!({
                "name": package_name,
                "version": "1.0.0",
                "scripts": {
                    "postinstall": "echo postinstall"
                }
            });
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .unwrap();
        }

        // Test scripts = false (should collect packages with scripts or binaries)
        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Run,
        )
        .await;
        assert!(result.is_ok());
        let packages_full = result.unwrap();
        assert_eq!(packages_full.len(), 3); // full-package, bin-only, script-only (no-hooks excluded)

        // Test scripts = true (should only collect packages with binaries)
        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Ignore,
        )
        .await;
        assert!(result.is_ok());
        let packages_bins_only = result.unwrap();
        assert_eq!(packages_bins_only.len(), 2); // full-package, bin-only (script-only and no-hooks excluded)

        // Verify the collected packages have correct bin_files
        for (package_info, _is_optional) in &packages_bins_only {
            assert!(
                !package_info.bin_files.is_empty(),
                "Package {} should have bin_files in scripts mode",
                package_info.name
            );
        }
    }

    /// Regression for #3097: a workspace produces two lock entries that both
    /// carry script/bin markers — the source node (`lib-a`) and the
    /// `node_modules` link (`node_modules/lib-a`). Their install lifecycle is
    /// owned by `process_workspace_install_hooks`, so `collect_packages_from_lock`
    /// must not re-queue their scripts (the source + link + workspace-walk =
    /// 3× run bug). The link is still collected for bin linking, and a plain
    /// `file:` link (not a workspace) keeps running its scripts.
    #[tokio::test]
    async fn test_collect_packages_from_lock_skips_workspace_hooks() {
        let temp_dir = TempDir::new().unwrap();
        let mut packages = HashMap::new();

        // Workspace source node — keyed by its root-relative dir (no node_modules).
        packages.insert(
            "lib-a".to_string(),
            LockPackage {
                name: Some("lib-a".to_string()),
                version: Some("1.0.0".to_string()),
                bin: Some(json!({"lib-a-cli": "bin/cli.js"})),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        // Workspace `node_modules` link — resolves back to the source dir. Link
        // nodes carry no script marker (the serializer omits has_install_script /
        // scripts on links since #3097); only `bin` + `link` + `resolved`.
        packages.insert(
            "node_modules/lib-a".to_string(),
            LockPackage {
                name: Some("lib-a".to_string()),
                link: Some(true),
                resolved: Some("lib-a".to_string()),
                bin: Some(json!({"lib-a-cli": "bin/cli.js"})),
                ..LockPackage::default()
            },
        );

        // A plain `file:` link — NOT a workspace (resolved points outside, with
        // no matching source entry). It carries no script marker either, so its
        // scripts must still be collected via the `is_link` gate exception and
        // read from disk.
        packages.insert(
            "node_modules/file-dep".to_string(),
            LockPackage {
                name: Some("file-dep".to_string()),
                link: Some(true),
                resolved: Some("../file-dep".to_string()),
                ..LockPackage::default()
            },
        );

        // A workspace whose dir name merely *contains* "node_modules" as a
        // substring (not a path segment). It must still be treated as a source
        // node and excluded — guards against a substring `contains` check.
        packages.insert(
            "node_modules-utils".to_string(),
            LockPackage {
                name: Some("node_modules-utils".to_string()),
                version: Some("1.0.0".to_string()),
                has_install_script: Some(true),
                ..LockPackage::default()
            },
        );

        let package_lock = PackageLock::new("ws".to_string(), "1.0.0".to_string(), packages);

        // Materialize package.json for every entry's on-disk path.
        for (rel, postinstall) in [
            ("lib-a", "echo source"),
            ("node_modules/lib-a", "echo link"),
            ("node_modules/file-dep", "echo file"),
            ("node_modules-utils", "echo substring-ws"),
        ] {
            let dir = temp_dir.path().join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("package.json"),
                serde_json::to_string_pretty(&json!({
                    "name": "x",
                    "version": "1.0.0",
                    "scripts": { "postinstall": postinstall }
                }))
                .unwrap(),
            )
            .unwrap();
        }

        let collected = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Run,
        )
        .await
        .unwrap();

        // The workspace source node (root-relative `lib-a`) is never collected.
        assert!(
            !collected
                .iter()
                .any(|(p, _)| p.path == temp_dir.path().join("lib-a")),
            "workspace source node must be excluded"
        );

        // A workspace dir containing "node_modules" as a substring is still a
        // source node and must be excluded (path-segment, not substring, check).
        assert!(
            !collected
                .iter()
                .any(|(p, _)| p.path == temp_dir.path().join("node_modules-utils")),
            "substring-named workspace source must be excluded"
        );

        // The workspace link is collected for bin linking but with no scripts.
        let link = collected
            .iter()
            .find(|(p, _)| p.path.to_string_lossy().contains("node_modules/lib-a"))
            .expect("workspace link should be collected for bin linking");
        assert!(
            !link.0.bin_files.is_empty(),
            "workspace link must keep its bin for node_modules/.bin"
        );
        assert!(
            link.0
                .lifecycle_scripts
                .get_script(LifecycleHook::Postinstall)
                .is_none(),
            "workspace link must not carry install scripts (owned by workspace walk)"
        );

        // The plain file: link keeps its postinstall.
        let file_dep = collected
            .iter()
            .find(|(p, _)| p.path.to_string_lossy().contains("file-dep"))
            .expect("file: link should still be collected");
        assert!(
            file_dep
                .0
                .lifecycle_scripts
                .get_script(LifecycleHook::Postinstall)
                .is_some(),
            "non-workspace file: link must keep running its scripts"
        );
    }

    /// A `file:` dep can serialize to a degenerate link entry (empty name →
    /// key `node_modules/`) whose path has no `package.json`. Reading scripts
    /// for it must not fail the whole install — collect tolerates the missing
    /// manifest for link nodes and yields no scripts. Regression for the
    /// `conflict-bundle-file-dep` e2e crash.
    #[tokio::test]
    async fn test_collect_tolerates_link_with_missing_manifest() {
        let temp_dir = TempDir::new().unwrap();
        // The link's path resolves to the node_modules dir itself, which exists
        // but holds no package.json.
        std::fs::create_dir_all(temp_dir.path().join("node_modules")).unwrap();

        let mut packages = HashMap::new();
        packages.insert(
            "node_modules/".to_string(),
            LockPackage {
                link: Some(true),
                resolved: Some("some-file-dep".to_string()),
                ..LockPackage::default()
            },
        );
        let package_lock = PackageLock::new("p".to_string(), "1.0.0".to_string(), packages);

        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Run,
        )
        .await;
        assert!(
            result.is_ok(),
            "degenerate link entry must not fail collect: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_collect_packages_from_lock_platform_compatibility() {
        let temp_dir = TempDir::new().unwrap();

        let mut packages = HashMap::new();

        // Package with incompatible OS
        packages.insert(
            "node_modules/win-only".to_string(),
            LockPackage {
                name: Some("win-only".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "tool.exe"})),
                has_install_script: Some(false),
                os: Some(json!(["win32"])), // Only Windows
                ..LockPackage::default()
            },
        );

        // Package with compatible platform
        packages.insert(
            "node_modules/cross-platform".to_string(),
            LockPackage {
                name: Some("cross-platform".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "tool.js"})),
                has_install_script: Some(false),
                ..LockPackage::default()
            },
        );

        let package_lock =
            PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages);

        // Create minimal package.json files
        let node_modules = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();

        {
            let package_name = &"cross-platform";
            // Only create compatible package
            let package_dir = node_modules.join(package_name);
            std::fs::create_dir_all(&package_dir).unwrap();

            let package_json = json!({
                "name": package_name,
                "version": "1.0.0"
            });
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .unwrap();
        }

        // Test that only compatible packages are collected
        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Ignore,
        )
        .await;
        assert!(result.is_ok());
        let packages_collected = result.unwrap();

        // Should only collect the cross-platform package (win-only filtered out by platform check)
        assert_eq!(packages_collected.len(), 1);
        assert_eq!(packages_collected[0].0.name, "cross-platform");
    }

    #[tokio::test]
    async fn test_collect_packages_from_lock_optional_flag() {
        let temp_dir = TempDir::new().unwrap();

        let mut packages = HashMap::new();

        // Regular (non-optional) package
        packages.insert(
            "node_modules/regular-pkg".to_string(),
            LockPackage {
                name: Some("regular-pkg".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "index.js"})),
                has_install_script: Some(false),
                optional: None,
                ..LockPackage::default()
            },
        );

        // Optional package
        packages.insert(
            "node_modules/optional-pkg".to_string(),
            LockPackage {
                name: Some("optional-pkg".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "index.js"})),
                has_install_script: Some(false),
                optional: Some(true),
                ..LockPackage::default()
            },
        );

        // Dev optional package
        packages.insert(
            "node_modules/dev-optional-pkg".to_string(),
            LockPackage {
                name: Some("dev-optional-pkg".to_string()),
                version: Some("1.0.0".to_string()),
                resolved: Some("registry-url".to_string()),
                bin: Some(json!({"tool": "index.js"})),
                has_install_script: Some(false),
                dev_optional: Some(true),
                ..LockPackage::default()
            },
        );

        let package_lock =
            PackageLock::new("test-project".to_string(), "1.0.0".to_string(), packages);

        // Create package directories
        let node_modules = temp_dir.path().join("node_modules");
        std::fs::create_dir_all(&node_modules).unwrap();

        for pkg_name in &["regular-pkg", "optional-pkg", "dev-optional-pkg"] {
            let package_dir = node_modules.join(pkg_name);
            std::fs::create_dir_all(&package_dir).unwrap();
            let package_json = json!({
                "name": pkg_name,
                "version": "1.0.0"
            });
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .unwrap();
        }

        // Collect packages
        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            ScriptPolicy::Ignore,
        )
        .await;
        assert!(result.is_ok());
        let packages_collected = result.unwrap();
        assert_eq!(packages_collected.len(), 3);

        // Verify is_optional flags are correctly set
        for (pkg_info, is_optional) in &packages_collected {
            match pkg_info.name.as_str() {
                "regular-pkg" => {
                    assert!(!is_optional, "regular-pkg should not be optional");
                }
                "optional-pkg" => {
                    assert!(is_optional, "optional-pkg should be optional");
                }
                "dev-optional-pkg" => {
                    assert!(is_optional, "dev-optional-pkg should be optional");
                }
                _ => panic!("Unexpected package: {}", pkg_info.name),
            }
        }
    }

    #[tokio::test]
    async fn test_execute_script_queue_optional_failure_ignored() {
        // Create a temporary directory for the test package
        let temp_dir = TempDir::new().unwrap();
        let package_path = temp_dir.path();

        // Create a package.json with a failing script
        let package_json = serde_json::json!({
            "name": "test-optional-fail",
            "version": "1.0.0",
            "scripts": {
                "postinstall": "exit 1"
            }
        });
        fs::write(
            package_path.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();

        // Create PackageInfo with failing script
        let package_info = PackageInfo {
            path: package_path.to_path_buf(),
            bin_files: vec![],
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::from_scripts(&HashMap::from([(
                "postinstall".to_string(),
                "exit 1".to_string(),
            )])),
            name: "test-optional-fail".to_string(),
        };

        // Test with is_optional = true: should NOT return error
        let queue_optional: Vec<(Rc<PackageInfo>, bool)> =
            vec![(Rc::new(package_info.clone()), true)];
        let result =
            PackageService::execute_script_queue(&queue_optional, LifecycleHook::Postinstall).await;
        assert!(
            result.is_ok(),
            "Optional dependency script failure should be ignored"
        );

        // Test with is_optional = false: should return error
        let queue_required: Vec<(Rc<PackageInfo>, bool)> = vec![(Rc::new(package_info), false)];
        let result =
            PackageService::execute_script_queue(&queue_required, LifecycleHook::Postinstall).await;
        assert!(
            result.is_err(),
            "Required dependency script failure should return error"
        );
    }

    #[tokio::test]
    async fn test_process_workspace_install_hooks_topological_order() {
        // Reproduces issue #2833: workspace B depends on A and consumes
        // A's `prepare`-built artifact. process_workspace_install_hooks must
        // run A's prepare before B's so B can find the artifact.
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::write(
            root.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["packages/*"]
            }"#,
        )
        .unwrap();

        let a_dir = root.join("packages/A");
        let b_dir = root.join("packages/B");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();

        // A's `prepare` produces lib/marker; B's `prepare` asserts it exists.
        fs::write(
            a_dir.join("package.json"),
            r#"{
                "name": "A",
                "version": "1.0.0",
                "scripts": {
                    "prepare": "mkdir -p lib && echo built > lib/marker"
                }
            }"#,
        )
        .unwrap();
        fs::write(
            b_dir.join("package.json"),
            r#"{
                "name": "B",
                "version": "1.0.0",
                "dependencies": { "A": "*" },
                "scripts": {
                    "prepare": "test -f ../A/lib/marker"
                }
            }"#,
        )
        .unwrap();

        let result = PackageService::process_workspace_install_hooks(root).await;
        assert!(
            result.is_ok(),
            "workspace install hooks should run in topological order: {result:?}"
        );
        assert!(
            a_dir.join("lib/marker").exists(),
            "A's prepare should have produced lib/marker"
        );
    }

    #[tokio::test]
    async fn test_process_workspace_install_hooks_no_workspaces() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Single package, no workspaces field — must be a no-op success.
        fs::write(
            root.join("package.json"),
            r#"{ "name": "lonely", "version": "1.0.0" }"#,
        )
        .unwrap();

        let result = PackageService::process_workspace_install_hooks(root).await;
        assert!(
            result.is_ok(),
            "non-workspace project should succeed without running anything"
        );
    }

    #[tokio::test]
    async fn test_process_workspace_install_hooks_propagates_failure() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::write(
            root.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["packages/*"]
            }"#,
        )
        .unwrap();

        let pkg_dir = root.join("packages/fail");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(
            pkg_dir.join("package.json"),
            r#"{
                "name": "fail",
                "version": "1.0.0",
                "scripts": { "prepare": "exit 1" }
            }"#,
        )
        .unwrap();

        let result = PackageService::process_workspace_install_hooks(root).await;
        assert!(
            result.is_err(),
            "a failing workspace prepare must surface as an error"
        );
    }

    #[tokio::test]
    async fn test_process_workspace_install_hooks_anonymous_workspaces() {
        // Regression for arborist `workspaces-need-update` fixture: workspace
        // package.json files may omit `name`. Without the folder-derived
        // fallback (npm `name-from-folder`), hooks could not be loaded and
        // anonymous siblings collapsed to the same empty key. We expect both
        // to be visited and to run their hooks.
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::write(
            root.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["a", "b"]
            }"#,
        )
        .unwrap();

        let a_dir = root.join("a");
        let b_dir = root.join("b");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();

        // Both workspaces are anonymous (no `name` field). Each writes a
        // distinct marker file from a `prepare` script.
        fs::write(
            a_dir.join("package.json"),
            r#"{ "scripts": { "prepare": "touch marker-a" } }"#,
        )
        .unwrap();
        fs::write(
            b_dir.join("package.json"),
            r#"{ "scripts": { "prepare": "touch marker-b" } }"#,
        )
        .unwrap();

        let result = PackageService::process_workspace_install_hooks(root).await;
        assert!(
            result.is_ok(),
            "anonymous workspaces should not fail load: {result:?}"
        );
        assert!(
            a_dir.join("marker-a").exists(),
            "anonymous workspace `a` prepare must have run"
        );
        assert!(
            b_dir.join("marker-b").exists(),
            "anonymous workspace `b` prepare must have run"
        );
    }
}
