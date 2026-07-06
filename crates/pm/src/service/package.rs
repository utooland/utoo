use crate::helper::ruborist_context::Context as FsContext;
use crate::model::package::{LifecycleHook, LifecycleScripts, PackageInfo};
use crate::util::install_progress::{mark_downloads_done, track_script};
use crate::util::logger::{PROGRESS_BAR, finish_progress_bar, log_progress, start_progress_bar};
use crate::util::script_policy::{
    InstallScriptMode, ScriptGateDecision, SkipReason, SkippedScript, report_skipped_scripts,
};
use crate::util::user_config::get_script_concurrency_limit;
use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use utoo_ruborist::compat::{is_cpu_compatible, is_os_compatible};
use utoo_ruborist::lock::{LockPackage, PackageLock};
use utoo_ruborist::manifest::ScriptsView;

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
    /// Packages whose install action was gated off by the `allowScripts` policy,
    /// for the end-of-install summary (and the strict-mode abort).
    pub skipped: Vec<SkippedScript>,
}

/// Max lifecycle scripts run concurrently within one queue. An explicit
/// `--script-concurrency-limit` / config value wins; otherwise (the `0` auto
/// default) it's ~one per core, capped at 16 — more conservative than the
/// I/O-bound clone gate's 2×, since each script is a child process and often a
/// CPU-bound native build, so over-subscribing the CPU would just thrash. The
/// lower bound tracks the hardware (no forced floor), so a single-core container
/// runs scripts serially rather than thrashing four at once.
async fn script_concurrency_limit() -> usize {
    let configured = get_script_concurrency_limit().await;
    if configured > 0 {
        configured
    } else {
        std::thread::available_parallelism().map_or(8, |n| n.get().clamp(1, 16))
    }
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
                LifecycleSink::Stream {
                    workspace_label,
                    timed: true,
                },
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
        mode: &InstallScriptMode,
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
                .map(|bin| bin.entries(&package_name))
                .unwrap_or_default();
            let has_bin = !bin_files.is_empty();

            // A workspace link contributes bin linking only — its install
            // scripts run via `process_workspace_install_hooks`. With no bin it
            // has nothing left to do here, so drop it.
            if is_workspace_link && !has_bin {
                continue;
            }

            if !Self::passes_script_policy(mode, has_scripts, has_bin, is_link) {
                continue;
            }

            if !Self::entry_platform_compatible(lock_package) {
                tracing::debug!("Package {path} is not compatible with current platform");
                continue;
            }

            let package_path = PathBuf::from(format!("{}/{}", root_path.display(), path));

            // Skip if package directory doesn't exist (e.g., omitted by --production/--omit)
            if !package_path.exists() {
                tracing::debug!("Package {path} not installed, skipping rebuild");
                continue;
            }

            let lifecycle_scripts = Self::entry_lifecycle_scripts(
                &package_path,
                path,
                mode,
                has_scripts,
                is_link,
                is_workspace_link,
            )
            .await?;

            // Check if this package is an optional dependency (based on edge type)
            let is_optional = lock_package.is_optional();

            let package_info = PackageInfo {
                path: package_path,
                bin_files,
                scripts: Default::default(),
                lifecycle_scripts,
                name: package_name,
                version: lock_package.version.clone().unwrap_or_default(),
                is_workspace_link,
            };

            packages.push((package_info, is_optional));
        }
        Ok(packages)
    }

    /// Early policy filter for one lock entry (npm's `#retrieveNodesByType`).
    ///
    /// A link node is never dropped on the `has_scripts` test — it carries no
    /// script marker in the lock; its scripts are read from disk afterwards.
    fn passes_script_policy(
        mode: &InstallScriptMode,
        has_scripts: bool,
        has_bin: bool,
        is_link: bool,
    ) -> bool {
        if mode.is_ignore_all() {
            // scripts-ignored mode: only packages with binaries matter.
            has_bin
        } else {
            // any script-running mode (allow-all or policy): packages with
            // scripts, binaries, or link nodes. Policy gating happens later, per
            // package, in `create_execution_queues_with_options`.
            has_scripts || has_bin || is_link
        }
    }

    /// Platform gate for one lock entry (absent os/cpu = compatible).
    fn entry_platform_compatible(lock_package: &LockPackage) -> bool {
        lock_package.cpu.as_ref().is_none_or(is_cpu_compatible)
            && lock_package.os.as_ref().is_none_or(is_os_compatible)
    }

    /// Read one entry's lifecycle scripts from its on-disk package.json, when
    /// the policy needs them.
    ///
    /// A workspace link contributes bin linking only — its scripts stay empty
    /// so `create_execution_queues` never queues them (they are owned by
    /// `process_workspace_install_hooks`). A `file:` link can resolve to a
    /// missing or degenerate `package.json` (e.g. a conflict artifact keyed
    /// `node_modules/` with an empty name); that reads as "no scripts" rather
    /// than failing the install, while a real dependency with an unreadable
    /// manifest still errors.
    ///
    /// A bin-only entry (`has_scripts` false and not a link) has no install
    /// scripts to read, so skip the `package.json` read entirely — otherwise
    /// every binary-bearing dependency in a large tree pays a needless disk read.
    /// Links are exempt: the serializer stamps no script marker on them
    /// (`has_scripts` is always false), so their scripts must be read from disk.
    async fn entry_lifecycle_scripts(
        package_path: &Path,
        path: &str,
        mode: &InstallScriptMode,
        has_scripts: bool,
        is_link: bool,
        is_workspace_link: bool,
    ) -> Result<LifecycleScripts> {
        if is_workspace_link || !mode.collects_scripts() || (!has_scripts && !is_link) {
            return Ok(LifecycleScripts::default());
        }
        match Self::read_lifecycle_scripts(package_path).await {
            Ok(s) => Ok(s),
            Err(_) if is_link => Ok(LifecycleScripts::default()),
            Err(e) => Err(e).with_context(|| format!("Failed to read scripts for package: {path}")),
        }
    }

    /// Create execution queues, applying the install-script policy per package.
    ///
    /// Bin linking is queued unconditionally (skipping a script never skips bin
    /// linking). Whether a package's lifecycle scripts are queued depends on
    /// `mode`:
    /// - [`InstallScriptMode::IgnoreAll`]: never (bin linking only).
    /// - [`InstallScriptMode::AllowAllDangerously`]: always (pre-RFC behavior;
    ///   no implicit `node-gyp` synthesis, to stay byte-for-byte compatible).
    /// - [`InstallScriptMode::Policy`]: gated by identity. Allowed native
    ///   packages get a synthesized `node-gyp rebuild` install action; skipped
    ///   or denied packages are recorded in `queues.skipped` and their scripts
    ///   left unqueued (so [`ScriptService::ensure_node_gyp`] is never reached
    ///   for an unallowed package).
    pub fn create_execution_queues_with_options(
        packages: Vec<(PackageInfo, bool)>,
        mode: &InstallScriptMode,
    ) -> Result<ExecutionQueues> {
        tracing::debug!("Creating execution queues with options...");
        let mut queues = ExecutionQueues::default();

        for (mut package, is_optional) in packages {
            let queue_scripts = Self::gate_package(mode, &mut package, &mut queues.skipped);
            let package = Rc::new(package);

            if queue_scripts {
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

    /// Apply the policy to one package, deciding whether its lifecycle scripts
    /// are queued and recording a skip when they are not.
    ///
    /// Returns `true` when the package's scripts should be queued. For an allowed
    /// native package with no explicit `install` script, synthesizes the implicit
    /// `node-gyp rebuild` action so it runs like npm's default install.
    fn gate_package(
        mode: &InstallScriptMode,
        package: &mut PackageInfo,
        skipped: &mut Vec<SkippedScript>,
    ) -> bool {
        let policy = match mode {
            InstallScriptMode::IgnoreAll => return false,
            InstallScriptMode::AllowAllDangerously => return true,
            InstallScriptMode::Policy(policy) => policy,
        };

        // Workspace links are first-party: their lifecycle is owned by the
        // workspace walk, so they are never gated as a dependency. (Their
        // `lifecycle_scripts` are already suppressed in `collect`; this also
        // prevents `is_node_gyp_pkg`'s on-disk probe — which follows the
        // node_modules symlink to the workspace source — from wrongly
        // synthesizing/gating a `node-gyp rebuild` and aborting a strict install
        // on a first-party workspace package.)
        if package.is_workspace_link {
            return true;
        }

        // An implicit `node-gyp rebuild` install action exists when the package
        // ships a `binding.gyp` and declares neither an `install` nor a
        // `preinstall` script — matching npm's default-install rule, so merely
        // allowing a package that runs its own `preinstall` setup does not add an
        // unexpected native build.
        let is_node_gyp = !package.lifecycle_scripts.suppresses_default_node_gyp()
            && ScriptService::is_node_gyp_pkg(package);
        let has_install_action = package.lifecycle_scripts.has_install_lifecycle() || is_node_gyp;
        if !has_install_action {
            // Nothing to gate (bin-only / no install action) — let it through;
            // queue construction adds no script entries for it anyway.
            return true;
        }

        match policy.decide(&package.name, &package.version) {
            ScriptGateDecision::Run => {
                if is_node_gyp {
                    // Materialize the implicit action so it is queued and run.
                    package
                        .lifecycle_scripts
                        .set(LifecycleHook::Install, "node-gyp rebuild".to_string());
                }
                true
            }
            ScriptGateDecision::Skip(reason) => {
                skipped.push(SkippedScript {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    reason,
                    node_gyp: is_node_gyp,
                });
                false
            }
            ScriptGateDecision::Error => {
                // Strict mode: recorded as unreviewed; the install is aborted in
                // `execute_queues_with_options` before any script runs.
                skipped.push(SkippedScript {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    reason: SkipReason::Unreviewed,
                    node_gyp: is_node_gyp,
                });
                false
            }
        }
    }

    /// Execute the queues, honoring the resolved [`InstallScriptMode`].
    ///
    /// Under a strict policy, any unreviewed install action aborts the install
    /// before a single script runs. The skip/fail summary is printed once at the
    /// end (or on abort).
    pub async fn execute_queues_with_options(
        queues: ExecutionQueues,
        mode: &InstallScriptMode,
    ) -> Result<()> {
        // The clone phase is over by the time scripts run, so stop the spinner's
        // network summary from carrying a frozen `↓ <total>` into this phase.
        mark_downloads_done();

        // Strict gate: fail fast on any unreviewed install action.
        if mode.is_strict()
            && queues
                .skipped
                .iter()
                .any(|s| s.reason == SkipReason::Unreviewed)
        {
            // Prints the skipped table AND the "how to allow them" hint; the
            // abort message then just points back to it.
            report_skipped_scripts(&queues.skipped);
            let count = queues
                .skipped
                .iter()
                .filter(|s| s.reason == SkipReason::Unreviewed)
                .count();
            anyhow::bail!(
                "install aborted by strict-allow-scripts: {count} package(s) with unreviewed \
                 install scripts (see above for how to allow them, or rerun without \
                 strict-allow-scripts)"
            );
        }

        if mode.is_ignore_all() {
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

        // Report anything the policy skipped (denied or unreviewed). Reached only
        // in non-strict runs, or strict runs with denies but no unreviewed.
        report_skipped_scripts(&queues.skipped);
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
                        let label = format!("{} {}", package.name, hook);
                        log_progress(&label);
                        // The renderer surfaces the longest-running entry with
                        // elapsed time, so a slow postinstall stays visible; its
                        // `tap` feeds output lines to the long-run heartbeat.
                        let running = track_script(label);
                        let start = std::time::Instant::now();
                        let result = ScriptService::execute_script(
                            &package,
                            hook,
                            ScriptOutput::Silent,
                            Some(running.sink()),
                        )
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

        // Run the queue with a concurrency gate rather than spawning every
        // script at once: each is a full child process (often a CPU-bound native
        // build like node-gyp), so an unbounded `join_all` on a big tree could
        // fork dozens-to-hundreds simultaneously and thrash the machine.
        let script_results: Vec<(bool, Result<()>)> = stream::iter(script_tasks)
            .buffer_unordered(script_concurrency_limit().await)
            .collect()
            .await;
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
    use utoo_ruborist::manifest::BinField;

    /// Test helper: build a single-binary `BinField::Map`.
    fn bin_map(name: &str, path: &str) -> Option<BinField> {
        Some(BinField::Map(std::collections::BTreeMap::from([(
            name.to_string(),
            path.to_string(),
        )])))
    }

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
            version: "1.0.0".to_string(),
            is_workspace_link: false,
        };

        // Prepare queues: only bin linking queue has this package
        // The bool indicates is_optional (false = not optional)
        let queues = ExecutionQueues {
            bin_linking: vec![(Rc::new(package_info), false)],
            ..Default::default()
        };

        // Should not panic or error, even though the bin file does not exist
        let result = PackageService::execute_queues_with_options(
            queues,
            &InstallScriptMode::AllowAllDangerously,
        )
        .await;
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
                bin: bin_map("cli", "bin/cli.js"),
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
                bin: bin_map("tool", "index.js"),
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
            &InstallScriptMode::AllowAllDangerously,
        )
        .await;
        assert!(result.is_ok());
        let packages_full = result.unwrap();
        assert_eq!(packages_full.len(), 3); // full-package, bin-only, script-only (no-hooks excluded)

        // Test scripts = true (should only collect packages with binaries)
        let result = PackageService::collect_packages_from_lock(
            &package_lock,
            temp_dir.path(),
            &InstallScriptMode::IgnoreAll,
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
                bin: bin_map("lib-a-cli", "bin/cli.js"),
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
                bin: bin_map("lib-a-cli", "bin/cli.js"),
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
            &InstallScriptMode::AllowAllDangerously,
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
            &InstallScriptMode::AllowAllDangerously,
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
                bin: bin_map("tool", "tool.exe"),
                has_install_script: Some(false),
                os: Some(serde_json::from_value(json!(["win32"])).unwrap()), // Only Windows
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
                bin: bin_map("tool", "tool.js"),
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
            &InstallScriptMode::IgnoreAll,
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
                bin: bin_map("tool", "index.js"),
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
                bin: bin_map("tool", "index.js"),
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
                bin: bin_map("tool", "index.js"),
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
            &InstallScriptMode::IgnoreAll,
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
            version: "1.0.0".to_string(),
            is_workspace_link: false,
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

    use crate::util::script_policy::AllowScriptsPolicy;

    /// Build a `PackageInfo` at `path` with the given lifecycle scripts.
    fn pkg_with_scripts(
        path: &Path,
        name: &str,
        version: &str,
        scripts: &[(&str, &str)],
    ) -> PackageInfo {
        let map: HashMap<String, String> = scripts
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        PackageInfo {
            path: path.to_path_buf(),
            bin_files: vec![],
            scripts: Default::default(),
            lifecycle_scripts: LifecycleScripts::from_scripts(&map),
            name: name.to_string(),
            version: version.to_string(),
            is_workspace_link: false,
        }
    }

    fn policy_mode(entries: &[(&str, bool)], strict: bool) -> InstallScriptMode {
        InstallScriptMode::Policy(AllowScriptsPolicy::from_entries(entries, strict))
    }

    /// Under a policy: allowed packages run, unreviewed are skipped+reported,
    /// denied are skipped+reported. Bin linking is unaffected (tested elsewhere).
    #[test]
    fn test_policy_gates_allow_deny_unreviewed() {
        let tmp = TempDir::new().unwrap();
        let packages = vec![
            (
                pkg_with_scripts(tmp.path(), "sharp", "1.0.0", &[("postinstall", "echo a")]),
                false,
            ),
            (
                pkg_with_scripts(tmp.path(), "evil", "2.0.0", &[("postinstall", "echo b")]),
                false,
            ),
            (
                pkg_with_scripts(tmp.path(), "telemetry", "3.0.0", &[("install", "echo c")]),
                false,
            ),
        ];
        let mode = policy_mode(&[("sharp", true), ("evil", false)], false);
        let queues = PackageService::create_execution_queues_with_options(packages, &mode).unwrap();

        // Only `sharp`'s postinstall is queued.
        assert_eq!(queues.postinstall.len(), 1);
        assert_eq!(queues.postinstall[0].0.name, "sharp");
        assert!(
            queues.install.is_empty(),
            "telemetry's install must not queue"
        );

        // `evil` (denied) + `telemetry` (unreviewed) are reported.
        assert_eq!(queues.skipped.len(), 2);
        let denied = queues.skipped.iter().find(|s| s.name == "evil").unwrap();
        assert_eq!(denied.reason, SkipReason::Denied);
        let unreviewed = queues
            .skipped
            .iter()
            .find(|s| s.name == "telemetry")
            .unwrap();
        assert_eq!(unreviewed.reason, SkipReason::Unreviewed);
    }

    /// A `binding.gyp` package with no explicit install script: when allowed, an
    /// implicit `node-gyp rebuild` install action is synthesized and queued;
    /// when unreviewed it is skipped and flagged as node-gyp.
    #[test]
    fn test_policy_gates_implicit_node_gyp() {
        // Allowed native package.
        let allowed = TempDir::new().unwrap();
        std::fs::write(allowed.path().join("binding.gyp"), "{}").unwrap();
        // Unreviewed native package (separate dir so both have binding.gyp).
        let unreviewed = TempDir::new().unwrap();
        std::fs::write(unreviewed.path().join("binding.gyp"), "{}").unwrap();

        let packages = vec![
            (
                pkg_with_scripts(allowed.path(), "native-ok", "1.0.0", &[]),
                false,
            ),
            (
                pkg_with_scripts(unreviewed.path(), "native-no", "2.1.0", &[]),
                false,
            ),
        ];
        let mode = policy_mode(&[("native-ok", true)], false);
        let queues = PackageService::create_execution_queues_with_options(packages, &mode).unwrap();

        // Allowed: implicit node-gyp rebuild synthesized into the install queue.
        assert_eq!(queues.install.len(), 1);
        assert_eq!(queues.install[0].0.name, "native-ok");
        assert_eq!(
            queues.install[0]
                .0
                .lifecycle_scripts
                .get_script(LifecycleHook::Install),
            Some("node-gyp rebuild")
        );

        // Unreviewed: skipped and flagged node-gyp.
        assert_eq!(queues.skipped.len(), 1);
        assert_eq!(queues.skipped[0].name, "native-no");
        assert!(queues.skipped[0].node_gyp, "must be flagged as node-gyp");
        assert_eq!(queues.skipped[0].reason, SkipReason::Unreviewed);
    }

    /// A `binding.gyp` package that declares its own `preinstall` (but no
    /// `install`) must NOT get an implicit `node-gyp rebuild` synthesized when
    /// allowed — npm suppresses the default install when `preinstall` exists, so
    /// enabling the package only runs its own preinstall, not an extra native build.
    #[test]
    fn test_preinstall_suppresses_implicit_node_gyp() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("binding.gyp"), "{}").unwrap();
        let packages = vec![(
            pkg_with_scripts(
                dir.path(),
                "native-pre",
                "1.0.0",
                &[("preinstall", "echo setup")],
            ),
            false,
        )];
        let mode = policy_mode(&[("native-pre", true)], false);
        let queues = PackageService::create_execution_queues_with_options(packages, &mode).unwrap();

        assert_eq!(queues.preinstall.len(), 1, "the declared preinstall runs");
        assert!(
            queues.install.is_empty(),
            "no implicit node-gyp install synthesized when preinstall is present"
        );
        assert!(queues.skipped.is_empty());
    }

    /// Allow-all-dangerously preserves the pre-RFC behavior: every explicit
    /// script runs and nothing is recorded as skipped (no node-gyp synthesis).
    #[test]
    fn test_allow_all_runs_everything_without_synthesis() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("binding.gyp"), "{}").unwrap();
        let packages = vec![(
            pkg_with_scripts(tmp.path(), "native", "1.0.0", &[("postinstall", "echo x")]),
            false,
        )];
        let queues = PackageService::create_execution_queues_with_options(
            packages,
            &InstallScriptMode::AllowAllDangerously,
        )
        .unwrap();
        assert_eq!(queues.postinstall.len(), 1);
        // No implicit node-gyp install action synthesized in allow-all mode.
        assert!(queues.install.is_empty());
        assert!(queues.skipped.is_empty());
    }

    /// A workspace `node_modules` link that ships a binding.gyp is first-party:
    /// it must NOT be gated as a dependency (its lifecycle is owned by the
    /// workspace walk), even under strict mode — otherwise a strict install
    /// would abort on a workspace package. Regression guard for the gate's
    /// on-disk node-gyp probe following the workspace symlink.
    #[test]
    fn test_workspace_link_with_binding_gyp_is_not_gated() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("binding.gyp"), "{}").unwrap();
        let mut pkg = pkg_with_scripts(tmp.path(), "ws-native", "1.0.0", &[]);
        pkg.is_workspace_link = true;
        pkg.bin_files = vec![("ws-cli".to_string(), "cli.js".to_string())];

        // Strict policy that does NOT list the workspace package.
        let mode = policy_mode(&[("other", true)], true);
        let queues =
            PackageService::create_execution_queues_with_options(vec![(pkg, false)], &mode)
                .unwrap();

        assert!(
            queues.skipped.is_empty(),
            "workspace link must not be recorded as a gated/skipped dependency"
        );
        assert_eq!(
            queues.bin_linking.len(),
            1,
            "workspace link must still be bin-linked"
        );
        assert!(
            queues.install.is_empty() && queues.preinstall.is_empty(),
            "workspace link must not have a synthesized node-gyp install queued"
        );
    }

    /// Strict mode aborts before running anything when a package is unreviewed.
    #[tokio::test]
    async fn test_strict_mode_aborts_on_unreviewed() {
        let tmp = TempDir::new().unwrap();
        let packages = vec![(
            pkg_with_scripts(
                tmp.path(),
                "telemetry",
                "1.0.0",
                &[("postinstall", "exit 1")],
            ),
            false,
        )];
        let mode = policy_mode(&[], true);
        let queues = PackageService::create_execution_queues_with_options(packages, &mode).unwrap();
        assert_eq!(queues.skipped.len(), 1);

        let result = PackageService::execute_queues_with_options(queues, &mode).await;
        assert!(
            result.is_err(),
            "strict mode must abort on unreviewed scripts"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("strict-allow-scripts"),
            "error should name the policy"
        );
    }
}
