//! Tool preparation belongs to installation. Script execution consumes paths only.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use utoo_ruborist::util::oncemap::OnceMap;

use super::{InstallService, bins};
use crate::helper::global_bin::{get_global_bin_dir, get_global_package_dir};
use crate::model::package::{LifecycleHook, PackageInfo};
use crate::service::script::{OutputSink, PreparedTools, ScriptOutput, ScriptService};
use crate::util::cli_enum::{InstallScope, ScriptPolicy};
use crate::util::user_config::{get_cache_dir, get_registry, get_supports_semver};

#[derive(Clone, Hash, PartialEq, Eq)]
struct PreparationKey {
    root: PathBuf,
    registry: String,
    cache: PathBuf,
    supports_semver: Option<bool>,
}

static PREPARED: LazyLock<OnceMap<PreparationKey, PreparedTools>> = LazyLock::new(OnceMap::new);
tokio::task_local! {
    static PREPARING: PathBuf;
}

/// This is the original hook trigger: only an existing lifecycle hook in a
/// package containing binding.gyp requests node-gyp. Project/run chains do not.
pub(crate) async fn execute_hook(
    executor: &ScriptService,
    package: &PackageInfo,
    hook: LifecycleHook,
    output: ScriptOutput,
    sink: Option<OutputSink>,
) -> Result<()> {
    if package.lifecycle_scripts.get_script(hook).is_none() {
        return Ok(());
    }
    let executor = if package.path.join("binding.gyp").exists() {
        // Boxing bounds the recursive future type: bootstrap hooks can request
        // tools, but receive the fully materialized provisional tool below.
        executor.with_tools(Box::pin(prepare_node_gyp(executor)).await?)
    } else {
        executor.clone()
    };
    executor.execute_script(package, hook, output, sink).await
}

async fn prepare_node_gyp(executor: &ScriptService) -> Result<PreparedTools> {
    if executor.tools().node_gyp.is_some() {
        return Ok(executor.tools().clone());
    }
    if let Some(program) = find_node_gyp(&executor.environment().path) {
        return Ok(PreparedTools {
            node_gyp: Some(program),
            ..Default::default()
        });
    }

    let prefix = executor.environment().prefix.as_deref();
    let root = get_global_package_dir(prefix)?.join("node-gyp");
    anyhow::ensure!(
        !PREPARING
            .try_with(|active| active == &root)
            .unwrap_or(false),
        "Cyclic tool preparation for node-gyp at {}",
        root.display()
    );
    let key = PreparationKey {
        root: root.clone(),
        registry: get_registry(),
        cache: get_cache_dir(),
        supports_semver: get_supports_semver(),
    };
    let ready = PREPARED
        .get_or_try_init(key, || {
            PREPARING.scope(root.clone(), async {
                let result = bootstrap_node_gyp(executor).await;
                if result.is_err() {
                    // A failed tool is never reused. Preserve the primary bootstrap error.
                    if let Err(error) = crate::fs::remove_dir_all(&root).await {
                        tracing::debug!("Failed to clean incomplete node-gyp: {error}");
                    }
                }
                result
            })
        })
        .await?;
    Ok((*ready).clone())
}

async fn bootstrap_node_gyp(executor: &ScriptService) -> Result<PreparedTools> {
    let prefix = executor.environment().prefix.as_deref();
    // Materialize the entire production tree before running any bootstrap hook.
    let installed = InstallService::materialize_global_package("node-gyp", prefix).await?;
    let package = PackageInfo::from_path(&installed.root).await?;
    let entry = package
        .bin_files
        .iter()
        .find(|(name, _)| name == "node-gyp")
        .context("Installed node-gyp does not provide a node-gyp executable")?;
    let program = installed.root.join(&entry.1);
    // A private bin directory makes node-gyp callable by its own and its
    // dependencies' hooks without advertising a successful global install yet.
    let bootstrap_bins = tempfile::tempdir()?;
    bins::link_to_target(&package, bootstrap_bins.path()).await?;
    let provisional = PreparedTools {
        node_gyp: Some(program.clone()),
        bin_dirs: vec![bootstrap_bins.path().to_path_buf()],
    };
    let mut environment = executor.environment().clone();
    environment.scope = InstallScope::Global;
    let bootstrap = ScriptService::new(environment).with_tools(provisional);
    InstallService::finish_global_package(
        installed,
        prefix,
        ScriptPolicy::Run,
        ScriptOutput::Silent,
        &bootstrap,
    )
    .await?;
    Ok(PreparedTools {
        node_gyp: Some(program),
        bin_dirs: vec![get_global_bin_dir(prefix)?],
    })
}

fn find_node_gyp(path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|dir| {
        #[cfg(windows)]
        let names = ["node-gyp.cmd", "node-gyp.exe", "node-gyp"];
        #[cfg(not(windows))]
        let names = ["node-gyp"];
        names
            .into_iter()
            .map(|name| dir.join(name))
            .find(|program| {
                let Ok(metadata) = program.metadata() else {
                    return false;
                };
                if !metadata.is_file() {
                    return false;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recursive_preparation_requires_provisional_tools() {
        use crate::service::script::ScriptEnvironment;
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().to_string_lossy().into_owned();
        let executor = ScriptService::new(ScriptEnvironment {
            init_cwd: dir.path().to_path_buf(),
            path: Default::default(),
            scope: InstallScope::Local,
            extra: Default::default(),
            prefix: Some(prefix.clone()),
        });
        let root = get_global_package_dir(Some(&prefix))
            .unwrap()
            .join("node-gyp");
        let result = PREPARING
            .scope(root.clone(), prepare_node_gyp(&executor))
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cyclic tool preparation")
        );
        let tools = PreparedTools {
            node_gyp: Some(root.join("bin/node-gyp.js")),
            bin_dirs: vec![],
        };
        let executor = executor.with_tools(tools);
        assert!(
            PREPARING
                .scope(root, prepare_node_gyp(&executor))
                .await
                .is_ok()
        );
    }

    #[test]
    fn path_lookup_rejects_directories_and_non_executables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().as_os_str();
        assert!(find_node_gyp(path).is_none());
        let program = dir.path().join("node-gyp");
        std::fs::create_dir(&program).unwrap();
        assert!(find_node_gyp(path).is_none());
        std::fs::remove_dir(&program).unwrap();
        std::fs::write(&program, "#!/bin/sh\nexit 0").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(find_node_gyp(path).is_none());
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(find_node_gyp(path), Some(program));
    }
}
