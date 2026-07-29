use std::path::Path;

use anyhow::{Context, Result};

use crate::service::dependency_graph::{LockGraphService, build_dep_tree};
use crate::util::format_print::print_dep_tree;

/// List dependency information, similar to npm list
pub async fn list_dependencies(cwd: &Path, package_name: &str) -> Result<()> {
    tracing::debug!("Starting dependency listing...");

    // Find package-lock.json file
    let lock_file_path = cwd.join("package-lock.json");
    if !crate::fs::try_exists(&lock_file_path).await? {
        return Err(anyhow::anyhow!(
            "package-lock.json not found in the current directory. \
             `utoo list` reads the dependency graph from the lockfile.\n\
             Run `utoo install` to generate one, then re-run `utoo list`."
        ));
    }

    // Load package-lock.json and build lock graph
    tracing::debug!("Loading package-lock.json and building lock graph...");
    let graph = LockGraphService::from_lock_file(&lock_file_path)
        .context("Failed to load and parse package-lock.json")?;

    show_package_dependencies(&graph, package_name)?;

    Ok(())
}

/// Display dependency information for a specific package
fn show_package_dependencies(graph: &LockGraphService, package_name: &str) -> Result<()> {
    let node_paths = graph.find_paths_to_root(package_name)?;
    if node_paths.is_empty() {
        tracing::debug!("No paths to root found");
        return Ok(());
    }
    let tree = build_dep_tree(&node_paths);
    print_dep_tree(&tree, graph, "", true, &[package_name]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use utoo_ruborist::lock::{LockPackage, LockPackageNode};

    use super::*;

    fn make_lock_package(name: &str, version: &str) -> LockPackage {
        LockPackage {
            name: Some(name.to_string()),
            version: Some(version.to_string()),
            ..Default::default()
        }
    }

    fn mock_graph() -> LockGraphService {
        let mut graph = LockGraphService::new();
        // root
        let root = LockPackageNode::new("".to_string(), make_lock_package("root", "1.0.0"));
        let a = LockPackageNode::new(
            "node_modules/a".to_string(),
            make_lock_package("a", "1.0.0"),
        );
        let b = LockPackageNode::new(
            "node_modules/b".to_string(),
            make_lock_package("b", "1.0.0"),
        );
        let c = LockPackageNode::new(
            "node_modules/c".to_string(),
            make_lock_package("c", "1.0.0"),
        );
        let root_idx = graph.add_package(root).unwrap();
        let a_idx = graph.add_package(a).unwrap();
        let b_idx = graph.add_package(b).unwrap();
        let c_idx = graph.add_package(c).unwrap();
        // root -> a, a -> b, b -> c
        graph.graph.add_edge(a_idx, root_idx, ());
        graph.graph.add_edge(b_idx, a_idx, ());
        graph.graph.add_edge(c_idx, b_idx, ());
        graph
    }

    #[test]
    fn test_show_package_dependencies_found() {
        let graph = mock_graph();
        // Should not panic or error
        let result = show_package_dependencies(&graph, "c");
        assert!(result.is_ok());
    }

    #[test]
    fn test_show_package_dependencies_not_found_includes_package_name() {
        let graph = mock_graph();
        let result = show_package_dependencies(&graph, "not-exist");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not-exist"),
            "error should mention the queried package name, got: {err}"
        );
        assert!(
            err.contains("utoo deps"),
            "error should suggest `utoo deps`, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_list_dependencies_lockfile_missing_error() {
        // A temp dir with no package-lock.json should produce a helpful error.
        let dir = tempfile::tempdir().unwrap();
        let result = list_dependencies(dir.path(), "some-package").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("package-lock.json"),
            "error should mention package-lock.json, got: {err}"
        );
        assert!(
            err.contains("utoo install"),
            "error should suggest `utoo install`, got: {err}"
        );
    }
}
