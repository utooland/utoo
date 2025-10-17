use crate::service::dependency_graph::DependencyGraphService;
use anyhow::{Context, Result};
use petgraph::graph::NodeIndex;
use std::collections::BTreeMap;
use std::path::Path;

use crate::model::package_lock::PackageLock;

/// List dependency information, similar to npm list
pub async fn list_dependencies(cwd: &Path, package_name: &str) -> Result<()> {
    tracing::debug!("Starting dependency listing...");

    // Find package-lock.json file
    let lock_file_path = cwd.join("package-lock.json");
    if !tokio::fs::try_exists(&lock_file_path).await? {
        return Err(anyhow::anyhow!(
            "package-lock.json not found in current directory"
        ));
    }

    // Load package-lock.json
    tracing::debug!("Loading package-lock.json...");
    let package_lock = PackageLock::from_lock_file(&lock_file_path)
        .context("Failed to parse package-lock.json")?;

    // Build dependency graph
    tracing::debug!("Building dependency graph...");
    let graph = package_lock
        .build_dependency_graph()
        .context("Failed to build dependency graph")?;

    show_package_dependencies(&graph, package_name)?;

    Ok(())
}

/// Display dependency information for a specific package
fn show_package_dependencies(graph: &DependencyGraphService, package_name: &str) -> Result<()> {
    let node_paths = graph.find_paths_to_root(package_name)?;
    if node_paths.is_empty() {
        tracing::debug!("No paths to root found");
        return Ok(());
    }
    let tree = build_dep_tree(&node_paths);
    print_dep_tree(&tree, graph, "", true, &[package_name]);
    Ok(())
}

#[derive(Debug)]
struct DepTreeNode {
    index: NodeIndex,
    children: BTreeMap<NodeIndex, DepTreeNode>,
}

fn build_dep_tree(paths: &[Vec<NodeIndex>]) -> DepTreeNode {
    let mut root = DepTreeNode {
        index: NodeIndex::end(),
        children: BTreeMap::new(),
    };
    for path in paths {
        let mut node = &mut root;
        for &idx in path.iter().rev() {
            node = node.children.entry(idx).or_insert_with(|| DepTreeNode {
                index: idx,
                children: BTreeMap::new(),
            });
        }
    }
    root
}

fn print_dep_tree(
    node: &DepTreeNode,
    graph: &DependencyGraphService,
    prefix: &str,
    is_last: bool,
    highlight: &[&str],
) {
    let is_root = node.index == NodeIndex::end();
    if !is_root {
        let branch = if is_last {
            "└──"
        } else {
            "├───┬"
        };
        if let Some(pkg) = graph.get_graph().node_weight(node.index) {
            let is_highlight = highlight.iter().any(|&h| pkg.name.starts_with(h));
            let display = format!("{}@{}", pkg.name, pkg.version);
            if is_highlight {
                if !pkg.path.is_empty() {
                    println!(
                        "{}{} \x1b[33m{}\x1b[0m -> {}",
                        prefix, branch, display, pkg.path
                    );
                } else {
                    println!("{prefix}{branch} \x1b[33m{display}\x1b[0m");
                }
            } else if !pkg.path.is_empty() {
                println!("{}{} {} -> {}", prefix, branch, display, pkg.path);
            } else {
                println!("{prefix}{branch} {display}");
            }
        }
    }
    let len = node.children.len();
    for (i, child) in node.children.values().enumerate() {
        let is_last_child = i == len - 1;
        let new_prefix = if is_root {
            String::new()
        } else {
            format!("{}{}", prefix, if is_last { "    " } else { "│   " })
        };
        print_dep_tree(child, graph, &new_prefix, is_last_child, highlight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::dependency_graph::{
        DependencyEdge, DependencyGraphService, DependencyType, PackageNode,
    };
    use petgraph::graph::NodeIndex;

    fn mock_graph() -> DependencyGraphService {
        let mut graph = DependencyGraphService::new();
        // root
        let root = PackageNode::new("root".to_string(), "1.0.0".to_string(), "".to_string());
        let a = PackageNode::new(
            "a".to_string(),
            "1.0.0".to_string(),
            "node_modules/a".to_string(),
        );
        let b = PackageNode::new(
            "b".to_string(),
            "1.0.0".to_string(),
            "node_modules/b".to_string(),
        );
        let c = PackageNode::new(
            "c".to_string(),
            "1.0.0".to_string(),
            "node_modules/c".to_string(),
        );
        let root_idx = graph.add_package(root).unwrap();
        let a_idx = graph.add_package(a).unwrap();
        let b_idx = graph.add_package(b).unwrap();
        let c_idx = graph.add_package(c).unwrap();
        // root -> a, a -> b, b -> c
        let edge = DependencyEdge {
            _dependency_type: DependencyType::Production,
            _version_spec: "1.0.0".to_string(),
        };
        graph.graph.add_edge(a_idx, root_idx, edge.clone());
        graph.graph.add_edge(b_idx, a_idx, edge.clone());
        graph.graph.add_edge(c_idx, b_idx, edge);
        graph
    }

    #[test]
    fn test_build_dep_tree() {
        let paths = vec![vec![
            NodeIndex::new(3),
            NodeIndex::new(2),
            NodeIndex::new(1),
            NodeIndex::new(0),
        ]];
        let tree = build_dep_tree(&paths);
        // root should have one child (a)
        assert_eq!(tree.children.len(), 1);
    }

    #[test]
    fn test_show_package_dependencies_found() {
        let graph = mock_graph();
        // Should not panic or error
        let result = show_package_dependencies(&graph, "c");
        assert!(result.is_ok());
    }

    #[test]
    fn test_show_package_dependencies_not_found() {
        let graph = mock_graph();
        let result = show_package_dependencies(&graph, "not-exist");
        assert!(result.is_err());
    }
}
