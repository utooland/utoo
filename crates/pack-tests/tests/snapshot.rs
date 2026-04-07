#![allow(clippy::needless_return)] // tokio macro-generated code doesn't respect this
#![allow(unexpected_cfgs)]
#![cfg(test)]

mod util;

use anyhow::{Context, Result};
use dunce::canonicalize;
use pack_api::{
    entrypoint::{
        EntrypointsWithIssues, all_output_assets_operation,
        get_all_written_entrypoints_with_issues_operation,
    },
    project::{ProjectContainer, ProjectOptions, WatchOptions},
};
use rustc_hash::FxHashSet;
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{Effects, OperationVc, ResolvedVc, TurboTasks, ValueToString, Vc, get_effects};
use turbo_tasks_backend::{BackendOptions, TurboTasksBackend, noop_backing_storage};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::Asset,
    issue::{CollectibleIssuesExt, IssueFilter},
    output::{OutputAsset, OutputAssetsReference},
};
use turbopack_test_utils::snapshot::{UPDATE, diff, expected, matches_expected, snapshot_issues};

use crate::util::REPO_ROOT;

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

    run(resource).unwrap();
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn run(resource: PathBuf) -> Result<()> {
    let tt = TurboTasks::new(TurboTasksBackend::new(
        BackendOptions {
            storage_mode: None,
            // Enable dependency tracking when we are running under UPDATE=1 to ensure file writes
            // don't crash the test.
            dependency_tracking: *UPDATE,
            ..Default::default()
        },
        noop_backing_storage(),
    ));
    tt.run(async move {
        #[turbo_tasks::function(operation)]
        async fn snapshot_issues_operation(out_op: OperationVc<FileSystemPath>) -> Result<Vc<()>> {
            let out_path = out_op
                .resolve()
                .strongly_consistent()
                .await?
                .owned()
                .await?;
            let plain_issues = out_op
                .peek_issues()
                .get_plain_issues(IssueFilter::everything())
                .await?;
            snapshot_issues(plain_issues, out_path.join("issues")?, &REPO_ROOT)
                .await
                .context("Unable to handle issues")?;
            Ok(Default::default())
        }

        #[turbo_tasks::function(operation)]
        async fn inner_operation(resource: RcStr) -> Result<Vc<Effects>> {
            let out_op = run_test_operation(resource);
            // Ensure bundling is complete before peeking issues or reading path.
            out_op.resolve().strongly_consistent().await?;

            // Run snapshot_issues as its own operation so its path.write() effects
            // are tracked as collectibles of snap_op (not of inner_operation).
            let snap_op = snapshot_issues_operation(out_op);
            snap_op.read_strongly_consistent().await?;

            // Collect and apply the snapshot write effects from within the operation
            // (calling get_effects / apply inside a turbo-tasks function is allowed).
            get_effects(snap_op).await?.apply().await?;

            Ok(get_effects(out_op).await?.cell())
        }
        inner_operation(resource.to_str().unwrap().into())
            .read_strongly_consistent()
            .await?
            .apply()
            .await?;

        Ok(())
    })
    .await?;

    Ok(())
}

#[turbo_tasks::function(operation)]
async fn run_test_operation(resource: RcStr) -> Result<Vc<FileSystemPath>> {
    let test_path = canonicalize(&resource)?;
    assert!(test_path.exists(), "{resource} does not exist");
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
    let (mut user_config, runtime_type_override): (serde_json::Value, Option<String>) =
        if config_content.trim().is_empty() {
            (serde_json::from_str(&default_config())?, None)
        } else {
            let raw_root: serde_json::Value = serde_json::from_str(&config_content)?;
            let runtime_type_from_root = raw_root
                .get("runtimeType")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let user_cfg = raw_root
                .get("config")
                .cloned()
                .expect("config.json must contain a top-level `config` object");
            (user_cfg, runtime_type_from_root)
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

        watch: WatchOptions::default(),
        dev: !is_production,
        build_id: "test".into(),
        pack_path: Path::new(&*REPO_ROOT)
            .join("node_modules/@utoo/pack")
            .to_string_lossy()
            .to_string()
            .into(),
    };

    // Initialize project container
    let container_op = ProjectContainer::new_operation(rcstr!("project"), project_options.dev);
    ProjectContainer::initialize(container_op, project_options).await?;
    let project_container = container_op.resolve().strongly_consistent().await?;

    // Run bundling operation using the same pattern as build.rs
    let entrypoints_with_issues_op =
        get_all_written_entrypoints_with_issues_operation(project_container);
    let EntrypointsWithIssues {
        entrypoints: _,
        issues: _,
        diagnostics: _,
        effects,
    } = &*entrypoints_with_issues_op
        .read_strongly_consistent()
        .await?;

    // Apply effects (write assets to disk)
    effects.apply().await?;

    // Get output assets and walk through them
    let project = project_container.project();
    let output_path = project.dist_root().owned().await?;

    // Get expected output files from the output directory
    let expected_paths = expected(output_path.clone()).await?;

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
        walk_asset(asset, &output_path, &mut seen, &mut queue)
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

async fn walk_asset(
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    output_path: &FileSystemPath,
    seen: &mut FxHashSet<FileSystemPath>,
    queue: &mut VecDeque<ResolvedVc<Box<dyn OutputAsset>>>,
) -> Result<()> {
    let path = asset.path().owned().await?;

    // Check if the path is already relative to output_path
    let full_path = if let Some(relative_path) = output_path.get_path_to(&path) {
        // Path is already inside output_path, join it
        output_path.join(relative_path)?
    } else {
        // Path is not inside output_path.
        // This might happen if it's a virtual path or if it's from a different FS.
        let path_str = path.to_string();
        if path_str.starts_with('/') {
            // If it's an absolute path, we only want the filename to avoid mirroring absolute paths
            // into the snapshot directory.
            let filename = Path::new(&path_str)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&path_str);
            output_path.join(filename)?
        } else {
            output_path.join(&path_str)?
        }
    };

    // Add the full path to seen set
    seen.insert(full_path.clone());
    diff(full_path, asset.content()).await?;

    queue.extend(asset.references().all_assets().await?.iter().copied());

    Ok(())
}
