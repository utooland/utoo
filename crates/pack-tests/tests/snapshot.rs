#![allow(clippy::needless_return)] // tokio macro-generated code doesn't respect this
#![allow(unexpected_cfgs)]
#![cfg(test)]

mod util;

use anyhow::{Context, Result};
use dunce::canonicalize;
use pack_api::{
    endpoint::get_written_endpoint_with_issues_operation,
    entrypoint::{
        EntrypointsWithIssues, all_output_assets_operation,
        get_all_written_entrypoints_with_issues_operation, get_entrypoints_with_issues_operation,
    },
    project::{ProjectContainer, ProjectOptions, WatchOptions},
};
use rustc_hash::FxHashSet;
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex},
};
use turbo_rcstr::rcstr;
use turbo_tasks::{
    Effects, OperationVc, ResolvedVc, TurboTasks, ValueToString, Vc,
    read_strongly_consistent_and_apply_effects, take_effects,
};
use turbo_tasks_backend::{BackendOptions, EvictionMode, TurboTasksBackend, noop_backing_storage};
use turbo_tasks_fs::{DirectoryContent, DirectoryEntry, FileSystemPath};
use turbopack_core::{
    asset::Asset,
    issue::{CollectibleIssuesExt, IssueFilter, IssueSeverity},
    output::{OutputAsset, OutputAssetsReference},
};
use turbopack_test_utils::snapshot::{UPDATE, diff, matches_expected, snapshot_issues};

use crate::util::REPO_ROOT;

static SNAPSHOT_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn default_config() -> String {
    r#"{
        "config": {
            "entry": [
                {
                    "import": "input/index.js",
                    "name": "main"
                }
            ],
            "output": {
                "path": "output"
            },
            "mode": "production",
            "optimization": {
                "minify": false,
                "moduleIds": "named"
            }
        },
        "runtimeType": "dummy"
    }"#
    .to_string()
}

fn is_empty_dir_tree(dir_entries: impl IntoIterator<Item = io::Result<fs::DirEntry>>) -> bool {
    for entry in dir_entries {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir()
            || !is_empty_dir_tree(fs::read_dir(entry.path()).unwrap())
        {
            return false;
        }
    }
    true
}

#[testing::fixture("tests/snapshot/*/*/", exclude("node_modules"))]
fn test(resource: PathBuf) {
    if !resource.exists() {
        return;
    }
    let resource = canonicalize(resource).unwrap();

    // Skip non-directory resources (like config.json files)
    if !resource.is_dir() {
        return;
    }

    let mut has_output_dir = false;
    let contents = fs::read_dir(&resource)
        .unwrap()
        .filter(|entry| {
            if entry.as_ref().unwrap().file_name() == "output" {
                has_output_dir = true;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();

    if is_empty_dir_tree(contents) {
        // a directory without output and config is not a test case
        if *UPDATE {
            fs::remove_dir_all(&resource).unwrap();
        } else if has_output_dir {
            let output_dir = resource.join("output");
            if !is_empty_dir_tree(fs::read_dir(output_dir).unwrap()) {
                panic!("{resource:?} contains a non-empty output directory, but no input files");
            }
        }
    }

    let _guard = SNAPSHOT_TEST_LOCK.lock().unwrap();
    run(resource.clone()).unwrap();
    run_assertion(&resource).unwrap();
}

fn run_assertion(resource: &Path) -> Result<()> {
    let assertion = resource.join("assert.js");
    if !assertion.try_exists()? {
        return Ok(());
    }

    let output = Command::new("node")
        .arg("assert.js")
        .current_dir(resource)
        .output()
        .context("Failed to execute snapshot assertion")?;

    anyhow::ensure!(
        output.status.success(),
        "Snapshot assertion failed for {}\nstdout:\n{}\nstderr:\n{}",
        resource.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn run(resource: PathBuf) -> Result<()> {
    let tt = TurboTasks::new(TurboTasksBackend::new(
        BackendOptions {
            storage_mode: None,
            // Enable dependency tracking when we are running under UPDATE=1 to ensure file writes
            // don't crash the test.
            dependency_tracking: *UPDATE,
            eviction_mode: EvictionMode::Off,
            ..Default::default()
        },
        noop_backing_storage(),
    ));
    tt.run_once(async move {
        let (project_options, write_endpoints_individually) =
            project_options_from_resource(&resource)?;
        let container_op = ProjectContainer::new_operation(rcstr!("project"), project_options.dev);
        ProjectContainer::initialize(container_op, project_options).await?;

        if write_endpoints_individually {
            let project_container = container_op.resolve().strongly_consistent().await?;
            let entrypoints_with_issues = read_strongly_consistent_and_apply_effects(
                get_entrypoints_with_issues_operation(project_container),
                |value| &value.effects,
            )
            .await?;

            for endpoint in entrypoints_with_issues
                .entrypoints
                .apps
                .iter()
                .chain(&entrypoints_with_issues.entrypoints.libraries)
            {
                let written_endpoint = get_written_endpoint_with_issues_operation(*endpoint)
                    .read_strongly_consistent()
                    .await?;
                let error_count = written_endpoint
                    .issues
                    .iter()
                    .filter(|issue| issue.severity <= IssueSeverity::Error)
                    .count();
                anyhow::ensure!(
                    error_count == 0,
                    "writing an endpoint to disk produced {error_count} error issue(s)"
                );
            }

            return Ok(());
        }

        #[turbo_tasks::function(operation, root)]
        async fn snapshot_issues_operation(out_op: OperationVc<FileSystemPath>) -> Result<Vc<()>> {
            let out_path = out_op
                .resolve()
                .strongly_consistent()
                .await?
                .owned()
                .await?;
            let plain_issues = out_op
                .peek_issues()
                .get_plain_issues(&IssueFilter::everything())
                .await?;
            snapshot_issues(plain_issues, out_path.join("issues")?, &REPO_ROOT)
                .await
                .context("Unable to handle issues")?;
            Ok(Default::default())
        }

        #[turbo_tasks::function(operation, root)]
        async fn snapshot_effects_operation(
            out_op: OperationVc<FileSystemPath>,
        ) -> Result<Vc<Effects>> {
            let snap_op = snapshot_issues_operation(out_op);
            snap_op.read_strongly_consistent().await?;
            Ok(take_effects(snap_op).await?.cell())
        }

        #[turbo_tasks::function(operation, root)]
        async fn output_effects_operation(
            out_op: OperationVc<FileSystemPath>,
        ) -> Result<Vc<Effects>> {
            out_op.resolve().strongly_consistent().await?;
            Ok(take_effects(out_op).await?.cell())
        }

        let out_op = run_test_operation(container_op);
        read_strongly_consistent_and_apply_effects(snapshot_effects_operation(out_op), |e| e)
            .await?;
        read_strongly_consistent_and_apply_effects(output_effects_operation(out_op), |e| e).await?;

        Ok(())
    })
    .await?;

    Ok(())
}

fn project_options_from_resource(resource: &Path) -> Result<(ProjectOptions, bool)> {
    let test_path = canonicalize(resource)?;
    assert!(test_path.exists(), "{} does not exist", resource.display());
    assert!(
        test_path.is_dir(),
        "{} is not a directory. Snapshot tests only support directories",
        test_path.to_string_lossy()
    );

    // Check if config.json exists in the current directory
    let config_path = test_path.join("config.json");
    let (project_path, config_content) = if config_path.exists() {
        // config.json is in the current directory, use current directory as project path
        let config_content = fs::read_to_string(&config_path)?;
        (test_path.clone(), config_content)
    } else {
        // config.json might be in the parent directory
        let parent_path = test_path.parent().unwrap();
        let parent_config_path = parent_path.join("config.json");
        if parent_config_path.exists() {
            // config.json is in the parent directory, use parent directory as project path
            let config_content = fs::read_to_string(&parent_config_path)?;
            (parent_path.to_path_buf(), config_content)
        } else {
            // No config.json found, use default config
            (test_path.clone(), default_config())
        }
    };

    // Parse config content and determine if it's in development or production mode
    let (mut user_config, runtime_type_override, watch_enabled, write_endpoints_individually): (
        serde_json::Value,
        Option<String>,
        bool,
        bool,
    ) = if config_content.trim().is_empty() {
        (serde_json::from_str(&default_config())?, None, false, false)
    } else {
        let raw_root: serde_json::Value = serde_json::from_str(&config_content)?;
        let runtime_type_from_root = raw_root
            .get("runtimeType")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let watch_enabled = raw_root
            .get("watch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let write_endpoints_individually = raw_root
            .get("writeEndpointsIndividually")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let user_cfg = raw_root
            .get("config")
            .cloned()
            .expect("config.json must contain a top-level `config` object");
        (
            user_cfg,
            runtime_type_from_root,
            watch_enabled,
            write_endpoints_individually,
        )
    };

    // Ensure default output configuration is present
    if user_config.get("output").is_none() {
        let default_output = serde_json::json!({
            "path": "output"
        });
        user_config["output"] = default_output;
    } else {
        let output = user_config.get("output").unwrap();
        if output.get("path").is_none() {
            user_config["output"]["path"] = serde_json::Value::String("output".to_string());
        }
    }

    // Ensure default mode is present
    if user_config.get("mode").is_none() {
        user_config["mode"] = serde_json::Value::String("production".to_string());
    }

    // Snapshot tests share a process-wide Node evaluation pool across test cases.
    // Use child processes so plugin transforms don't reuse worker-thread state
    // between concurrently running fixtures.
    if user_config.get("pluginRuntimeStrategy").is_none() {
        user_config["pluginRuntimeStrategy"] =
            serde_json::Value::String("childProcesses".to_string());
    }

    // Ensure optimization is present (minify default to false for snapshots)
    if user_config.get("optimization").is_none() {
        let default_optimization = serde_json::json!({
            "minify": false,
            "moduleIds": "named",
        });
        user_config["optimization"] = default_optimization;
    }

    // Propagate runtimeType into inner bundler config so downstream can read it
    if let Some(rt) = &runtime_type_override {
        user_config["runtimeType"] = serde_json::Value::String(rt.clone());
    }

    let is_production = user_config
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|m| m == "production")
        .unwrap_or(true);

    // Convert the merged inner bundler config back to string for ProjectOptions
    let final_config_content = serde_json::to_string_pretty(&user_config)?;

    let project_options = ProjectOptions {
        root_path: Path::new(&*REPO_ROOT)
            .join("crates/pack-tests/tests/snapshot")
            .to_string_lossy()
            .to_string()
            .into(),
        project_path: project_path.to_string_lossy().into(),
        config: final_config_content.into(),
        process_env: vec![
            (
                "NODE_ENV".into(),
                if is_production {
                    "production".into()
                } else {
                    "development".into()
                },
            ),
            ("TURBOPACK".into(), "1".into()),
        ],

        watch: WatchOptions {
            enable: watch_enabled,
            ..Default::default()
        },
        dev: !is_production,
        build_id: "test".into(),
        pack_path: Path::new(&*REPO_ROOT)
            .join("node_modules/@utoo/pack")
            .to_string_lossy()
            .to_string()
            .into(),
    };

    Ok((project_options, write_endpoints_individually))
}

#[turbo_tasks::function(operation, root)]
async fn run_test_operation(
    container_op: OperationVc<ProjectContainer>,
) -> Result<Vc<FileSystemPath>> {
    let project_container = container_op.resolve().strongly_consistent().await?;

    // Run bundling operation using the same pattern as build.rs
    let entrypoints_with_issues_op =
        get_all_written_entrypoints_with_issues_operation(project_container);
    let EntrypointsWithIssues {
        entrypoints: _,
        issues: _,
        effects: _,
    } = &*entrypoints_with_issues_op
        .read_strongly_consistent()
        .await?;

    // Get output assets and walk through them
    let project = project_container.project();
    let dist_root = project.dist_root().owned().await?;
    let server_dist_root = project.server_dist_root().owned().await?;

    // Determine snapshot comparison root:
    // If server output is inside client dist_root (e.g. dist/ + dist/server),
    // use dist_root. Otherwise they're siblings (e.g. output/client + output/server),
    // so use the parent directory.
    let output_path = if server_dist_root.is_inside_ref(&dist_root) {
        dist_root.clone()
    } else {
        dist_root.parent()
    };

    // Get expected output files from the output directory (recursive)
    let expected_paths = expected_recursive(output_path.clone()).await?;

    let output_assets = all_output_assets_operation(project_container)
        .connect()
        .await?;

    // Walk through all assets for snapshot comparison
    let mut seen = FxHashSet::default();
    let mut queue = VecDeque::new();

    // Add all output assets to queue
    for asset in output_assets.iter() {
        queue.push_back(*asset);
    }

    // Process all assets
    while let Some(asset) = queue.pop_front() {
        walk_asset(asset, &output_path, &dist_root, &mut seen, &mut queue)
            .await
            .context(format!(
                "Failed to walk asset {}",
                asset.path().to_string().await.context("to_string failed")?
            ))?;
    }

    // Verify that actual assets match expected assets
    matches_expected(expected_paths, seen)
        .await
        .context("Actual assets don't match with expected assets")?;

    // Return the project path for further processing
    Ok(project.project_path().owned().await?.cell())
}

/// Like `expected()` from turbopack_test_utils but recursively descends into subdirectories.
async fn expected_recursive(dir: FileSystemPath) -> Result<FxHashSet<FileSystemPath>> {
    let mut result = FxHashSet::default();
    let entries = dir.read_dir().await?;
    if let DirectoryContent::Entries(entries) = &*entries {
        for (_name, entry) in entries {
            match entry {
                DirectoryEntry::File(file) => {
                    result.insert(file.clone());
                }
                DirectoryEntry::Directory(sub_dir) => {
                    let sub_files = Box::pin(expected_recursive(sub_dir.clone())).await?;
                    result.extend(sub_files);
                }
                _ => {} // skip symlinks etc.
            }
        }
    }
    Ok(result)
}

async fn walk_asset(
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    output_path: &FileSystemPath,
    dist_root: &FileSystemPath,
    seen: &mut FxHashSet<FileSystemPath>,
    queue: &mut VecDeque<ResolvedVc<Box<dyn OutputAsset>>>,
) -> Result<()> {
    let path = asset.path().owned().await?;

    let full_path = if let Some(relative_path) = output_path.get_path_to(&path) {
        // Path is on the output FS, inside output_path (e.g. server chunks)
        output_path.join(relative_path)?
    } else {
        // Path is on a different FS (client virtual FS).
        // Rebase to dist_root to match real emit behavior.
        let path_str = path.to_string();
        if path_str.starts_with('/') {
            let filename = Path::new(&path_str)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&path_str);
            dist_root.join(filename)?
        } else {
            dist_root.join(&path_str)?
        }
    };

    // Add the full path to seen set
    seen.insert(full_path.clone());
    diff(full_path, asset.content()).await?;

    queue.extend(asset.references().all_assets().await?.iter().copied());

    Ok(())
}
