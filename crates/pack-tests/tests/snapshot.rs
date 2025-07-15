#![allow(clippy::needless_return)] // tokio macro-generated code doesn't respect this
#![cfg(test)]

mod util;

use anyhow::{Context, Result};
use dunce::canonicalize;
use pack_api::{
    entrypoint::{
        EntrypointsWithIssues, all_output_assets_operation,
        get_all_written_entrypoints_with_issues_operation,
    },
    project::{DefineEnv, ProjectContainer, ProjectOptions, WatchOptions},
};
use rustc_hash::FxHashSet;
use std::{collections::VecDeque, fs, io, path::PathBuf, sync::Once};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ReadConsistency, ResolvedVc, TurboTasks, ValueToString, Vc, apply_effects};
use turbo_tasks_backend::{BackendOptions, TurboTasksBackend, noop_backing_storage};
use turbo_tasks_fs::{FileSystemPath, util::sys_to_unix};
use turbopack_core::{asset::Asset, issue::IssueDescriptionExt, output::OutputAsset};
use turbopack_test_utils::snapshot::{UPDATE, diff, expected, matches_expected, snapshot_issues};

use crate::util::REPO_ROOT;

fn register() {
    turbo_tasks::register();
    turbo_tasks_env::register();
    turbo_tasks_fs::register();
    turbopack::register();
    turbopack_nodejs::register();
    turbopack_browser::register();
    turbopack_ecmascript_plugins::register();
    turbopack_ecmascript_runtime::register();
    turbopack_resolve::register();
    turbopack_core::register();
    pack_core::register();
    include!(concat!(env!("OUT_DIR"), "/register_test_snapshot.rs"));
}

fn default_config() -> String {
    r#"{
        "entry": [
            {
                "import": "input/index.js",
                "name": "main"
            }
        ],
        "output": {
            "path": "output"
        },
        "mode": "production"
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

#[tokio::main(flavor = "current_thread")]
async fn run(resource: PathBuf) -> Result<()> {
    static REGISTER_ONCE: Once = Once::new();
    REGISTER_ONCE.call_once(register);

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
    let task = tt.spawn_once_task(async move {
        let emit_op = run_inner_options(resource.to_str().unwrap().into());
        emit_op.read_strongly_consistent().await?;
        apply_effects(emit_op).await?;
        Ok(Vc::<()>::default())
    });
    tt.wait_task_completion(task, ReadConsistency::Strong)
        .await?;

    Ok(())
}

#[turbo_tasks::function(operation)]
async fn run_inner_options(resource: RcStr) -> Result<()> {
    let output_op = run_test_operation(resource);
    let out_vc = output_op
        .resolve_strongly_consistent()
        .await?
        .await?
        .clone_value();
    let captured_issues = output_op.peek_issues_with_path().await?;

    let plain_issues = captured_issues.get_plain_issues().await?;

    snapshot_issues(plain_issues, out_vc.join("issues")?, &REPO_ROOT)
        .await
        .context("Failed to handle issues")?;

    Ok(())
}

#[turbo_tasks::function(operation)]
async fn run_test_operation(resource: RcStr) -> Result<Vc<FileSystemPath>> {
    // Register pack-api functions
    pack_api::register();

    let test_path = canonicalize(&resource)?;
    assert!(test_path.exists(), "{resource} does not exist");
    assert!(
        test_path.is_dir(),
        "{} is not a directory. Snapshot tests only support directories",
        test_path.to_str().unwrap()
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
    let config_json: serde_json::Value = if config_content.trim().is_empty() {
        serde_json::from_str(&default_config())?
    } else {
        let mut user_config: serde_json::Value = serde_json::from_str(&config_content)?;

        // Ensure default output configuration is present
        if !user_config.get("output").is_some() {
            let default_output = serde_json::json!({
                "path": "output"
            });
            user_config["output"] = default_output;
        }

        // Ensure default mode is present
        if !user_config.get("mode").is_some() {
            user_config["mode"] = serde_json::Value::String("production".to_string());
        }

        // Ensure minify is default to true
        if !user_config.get("optimization").is_some() {
            let default_optimization = serde_json::json!({
                "minify": false,
            });
            user_config["optimization"] = default_optimization;
        }

        user_config
    };

    let is_production = config_json
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|m| m == "production")
        .unwrap_or(true);

    // Convert the merged config back to string for ProjectOptions
    let final_config_content = serde_json::to_string_pretty(&config_json)?;

    let project_options = ProjectOptions {
        root_path: REPO_ROOT.to_string().into(),
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
        define_env: DefineEnv::default(),
        watch: WatchOptions::default(),
        dev: !is_production,
        build_id: "test".into(),
    };

    let relative_path = test_path.strip_prefix(&*REPO_ROOT)?;
    let relative_path: RcStr = sys_to_unix(relative_path.to_str().unwrap()).into();
    let _project_path = project_path.join(relative_path);

    // Initialize project container
    let project_container_vc = ProjectContainer::new(rcstr!("project"), project_options.dev);
    let project_container_resolved = project_container_vc.to_resolved().await?;
    project_container_resolved
        .initialize(project_options)
        .await?;

    // Run bundling operation using the same pattern as build.rs
    let entrypoints_with_issues_op =
        get_all_written_entrypoints_with_issues_operation(project_container_resolved);
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
    let project = project_container_vc.project();
    let output_path = project.dist_root().await?.clone_value();

    // Get expected output files from the output directory
    let expected_paths = expected(output_path.clone()).await?;

    let output_assets = all_output_assets_operation(project_container_resolved)
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

    // dbg!(&expected_paths, &seen);

    // Verify that actual assets match expected assets
    matches_expected(expected_paths, seen)
        .await
        .context("Actual assets don't match with expected assets")?;

    // Return the project path for further processing
    Ok(project.project_path().await?.clone_value().cell())
}

async fn walk_asset(
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    output_path: &FileSystemPath,
    seen: &mut FxHashSet<FileSystemPath>,
    queue: &mut VecDeque<ResolvedVc<Box<dyn OutputAsset>>>,
) -> Result<()> {
    let path = asset.path().await?.clone_value();

    // Check if the path is already relative to output_path
    let full_path = if let Some(_relative_path) = output_path.get_path_to(&path) {
        // Path is already inside output_path, use it directly
        path.clone()
    } else {
        // Path is not inside output_path, join it
        output_path.join(&path.to_string())?
    };

    // Add the full path to seen set
    seen.insert(full_path.clone());
    diff(full_path, asset.content()).await?;

    queue.extend(
        asset
            .references()
            .await?
            .iter()
            .copied()
            .flat_map(ResolvedVc::try_downcast::<Box<dyn OutputAsset>>),
    );

    Ok(())
}
