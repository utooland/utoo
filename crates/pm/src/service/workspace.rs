use crate::helper::deps::{compute_topological_layers, find_cycle_groups};
use crate::helper::tree_builder::TreeBuilder;
use crate::util::cache::matches_pattern;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use utoo_ruborist::graph::EdgeType;

/// Normalized workspace selection.
///
/// Both `--workspace` (repeatable, supports globs) and `--workspaces` (all)
/// CLI inputs collapse into one of these variants. The service layer never
/// sees the raw flags.
#[derive(Debug, Clone)]
pub enum WorkspaceFilter {
    /// No workspace targeting; operate on the current cwd's package.
    Current,
    /// Operate on every workspace, ordered by the workspace topology.
    All,
    /// Operate on the listed workspaces (names, relative paths, or glob patterns).
    Selected(Vec<String>),
}

impl WorkspaceFilter {
    /// Build a filter from the two CLI flags. `--workspaces` (all) takes
    /// precedence; an empty `workspace` list means "current package".
    pub fn from_flags(workspace: Vec<String>, workspaces: bool) -> Self {
        if workspaces {
            Self::All
        } else if workspace.is_empty() {
            Self::Current
        } else {
            Self::Selected(workspace)
        }
    }
}

/// Outcome of resolving a [`WorkspaceFilter`] against the project on disk.
#[derive(Debug, Clone)]
pub enum ResolvedWorkspaces {
    /// Operate on the current cwd's package only.
    Current,
    /// Operate on the given topological layers (each inner Vec runs concurrently).
    /// Includes a name→path map so callers skip redundant workspace discovery.
    Layers {
        layers: Vec<Vec<String>>,
        paths: HashMap<String, PathBuf>,
    },
}

/// Workspace dependency edge
#[derive(Debug, Clone)]
pub struct WorkspaceEdge {
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
}

/// Workspace node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceNode {
    pub name: String,
    pub path: PathBuf,
}

/// Workspace topology information
#[derive(Debug, Clone)]
pub struct WorkspaceTopology {
    pub edges: Vec<WorkspaceEdge>,
    pub topology: Vec<Vec<String>>,
    pub nodes: Vec<WorkspaceNode>,
}

/// Service for workspace operations
pub struct WorkspaceService;

impl WorkspaceService {
    /// Build workspace topology by analyzing dependencies between workspaces
    pub async fn build_workspace_topology(cwd: &Path) -> Result<WorkspaceTopology> {
        let mut builder = TreeBuilder::new(cwd);
        builder.build_workspace_tree().await?;

        let Some(graph) = &builder.ideal_tree else {
            return Ok(WorkspaceTopology {
                edges: Vec::new(),
                topology: Vec::new(),
                nodes: Vec::new(),
            });
        };

        // Get all workspace nodes (excluding links)
        let workspace_nodes = graph.get_workspace_nodes();

        let mut node_list = Vec::with_capacity(workspace_nodes.len());
        let mut nodes = Vec::with_capacity(workspace_nodes.len());
        let mut edges = Vec::new();
        let mut workspace_names = HashSet::with_capacity(workspace_nodes.len());

        for node_idx in &workspace_nodes {
            let node = graph
                .get_node(*node_idx)
                .expect("workspace node index must be valid");
            let name = node.name.clone();

            workspace_names.insert(name.clone());
            node_list.push(name.clone());
            nodes.push(WorkspaceNode {
                name,
                path: node
                    .path
                    .strip_prefix(cwd)
                    .unwrap_or(&node.path)
                    .to_path_buf(),
            });
        }

        // Collect dependency edges between workspaces
        for (i, node_idx) in workspace_nodes.iter().enumerate() {
            let node_name = &nodes[i].name;

            for (_, dep) in graph.get_dependency_edges(*node_idx) {
                if !dep.valid {
                    continue;
                }
                let Some(target_idx) = dep.to else {
                    continue;
                };
                let target_node = graph
                    .get_node(target_idx)
                    .expect("target node index must be valid");
                if workspace_names.contains(&target_node.name) {
                    edges.push(WorkspaceEdge {
                        from: target_node.name.clone(),
                        to: node_name.clone(),
                        edge_type: dep.edge_type,
                    });
                }
            }
        }

        // Detect cycles once; if any exist, drop dev edges inside the
        // same SCC before running the topological sort.
        let all_pairs: Vec<_> = edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        let cycles = find_cycle_groups(&node_list, &all_pairs);
        let topology = if cycles.is_empty() {
            compute_topological_layers(&node_list, &all_pairs)?
        } else {
            let mut node_to_group = HashMap::new();
            for (i, group) in cycles.iter().enumerate() {
                for name in group {
                    node_to_group.insert(name.as_str(), i);
                }
            }
            let filtered: Vec<_> = edges
                .iter()
                .filter(|e| {
                    !(e.edge_type == EdgeType::Dev
                        && node_to_group.contains_key(e.from.as_str())
                        && node_to_group.get(e.from.as_str()) == node_to_group.get(e.to.as_str()))
                })
                .map(|e| (e.from.as_str(), e.to.as_str()))
                .collect();
            compute_topological_layers(&node_list, &filtered)?
        };

        Ok(WorkspaceTopology {
            edges,
            topology,
            nodes,
        })
    }

    /// Resolve a workspace filter into the layers we will operate on.
    ///
    /// - `Current` → no targeting.
    /// - `All` → full project topology.
    /// - `Selected` → match each entry (name / relative path / glob) against
    ///   the workspace nodes carried by the topology, then keep only those
    ///   names from the layers so dependency order is preserved across the
    ///   selected subset.
    pub async fn resolve_layers(cwd: &Path, filter: WorkspaceFilter) -> Result<ResolvedWorkspaces> {
        match filter {
            WorkspaceFilter::Current => Ok(ResolvedWorkspaces::Current),
            WorkspaceFilter::All | WorkspaceFilter::Selected(_) => {
                let topo = Self::build_workspace_topology(cwd).await?;
                if topo.topology.is_empty() && matches!(filter, WorkspaceFilter::All) {
                    return Ok(ResolvedWorkspaces::Current);
                }

                // Build name→absolute-path map from topology nodes.
                let paths: HashMap<String, PathBuf> = topo
                    .nodes
                    .iter()
                    .map(|n| (n.name.clone(), cwd.join(&n.path)))
                    .collect();

                let layers = match filter {
                    WorkspaceFilter::All => topo.topology,
                    WorkspaceFilter::Selected(ref filters) => {
                        let selected = expand_filters(&topo.nodes, filters)?;
                        project_topology_layers(&topo.topology, &selected)
                    }
                    WorkspaceFilter::Current => unreachable!(),
                };

                Ok(ResolvedWorkspaces::Layers { layers, paths })
            }
        }
    }

    /// Build workspace topology and generate the typed `workspace.json` output.
    pub async fn build_workspace_json(cwd: &Path) -> Result<WorkspaceJson> {
        let topology = Self::build_workspace_topology(cwd).await?;
        Ok(WorkspaceJson {
            node_list: topology.nodes,
            edges: topology
                .edges
                .into_iter()
                .map(|edge| [edge.from, edge.to])
                .collect(),
            topology: topology.topology,
        })
    }
}

/// On-disk `workspace.json`: the workspace dependency graph as node list,
/// `[from, to]` edge pairs, and topological layers. Field declaration order
/// is the serialized key order.
#[derive(Debug, Serialize)]
pub struct WorkspaceJson {
    #[serde(rename = "nodeList")]
    pub node_list: Vec<WorkspaceNode>,
    pub edges: Vec<[String; 2]>,
    pub topology: Vec<Vec<String>>,
}

/// Expand user-provided workspace filters into a deduplicated set of
/// canonical workspace names.
///
/// Each filter is matched against the workspace name and its project-relative
/// path via [`matches_pattern`], which handles both literal equality and the
/// `*` glob forms supported elsewhere in the codebase (e.g. `packages/*`).
fn expand_filters(nodes: &[WorkspaceNode], filters: &[String]) -> Result<BTreeSet<String>> {
    // Stringify each node's relative path once instead of per filter.
    let node_paths: Vec<(&str, String)> = nodes
        .iter()
        .map(|n| (n.name.as_str(), n.path.to_string_lossy().into_owned()))
        .collect();

    let mut selected = BTreeSet::new();
    for filter in filters {
        let mut matched = false;
        for (name, path) in &node_paths {
            if matches_pattern(name, filter) || matches_pattern(path, filter) {
                selected.insert((*name).to_string());
                matched = true;
            }
        }
        if !matched {
            anyhow::bail!("Workspace filter '{filter}' did not match any workspace");
        }
    }

    Ok(selected)
}

/// Project the full workspace topology onto a selected subset, preserving
/// the original layer ordering and dropping empty layers.
fn project_topology_layers(
    topology: &[Vec<String>],
    selected: &BTreeSet<String>,
) -> Vec<Vec<String>> {
    topology
        .iter()
        .map(|layer| {
            layer
                .iter()
                .filter(|name| selected.contains(name.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        })
        .filter(|layer| !layer.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Create a two-workspace (A, B) mock structure with custom package.json content.
    fn create_mock_workspace_with(dir: &Path, a_json: &str, b_json: &str) {
        let a_dir = dir.join("A");
        let b_dir = dir.join("B");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{
                "name": "root",
                "private": true,
                "workspaces": ["A", "B"]
            }"#,
        )
        .unwrap();
        fs::write(a_dir.join("package.json"), a_json).unwrap();
        fs::write(b_dir.join("package.json"), b_json).unwrap();
    }

    fn create_mock_workspace(dir: &Path) {
        create_mock_workspace_with(
            dir,
            r#"{"name":"A"}"#,
            r#"{"name":"B","dependencies":{"A":"*"}}"#,
        );
    }

    #[tokio::test]
    async fn test_dev_deps_cycle_falls_back() {
        // A devDepends on B, B devDepends on A — cycle via devDeps only.
        // Should fall back to omitting dev edges instead of erroring.
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","devDependencies":{"B":"*"}}"#,
            r#"{"name":"B","devDependencies":{"A":"*"}}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok(), "devDeps cycle should fall back, not error");
        let topo = result.unwrap();
        assert_eq!(topo.edges.len(), 2);
        // Fallback: no prod edges → no ordering constraints → single layer
        assert_eq!(topo.topology.len(), 1);
        assert_eq!(topo.topology[0].len(), 2);
    }

    #[tokio::test]
    async fn test_dev_deps_affect_ordering_when_no_cycle() {
        // A devDepends on B (no cycle) — devDep should still affect ordering
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","devDependencies":{"B":"*"}}"#,
            r#"{"name":"B"}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok());
        let topo = result.unwrap();
        // B first, then A (A devDepends on B → B should be built first)
        assert_eq!(topo.topology, vec![vec!["B"], vec!["A"]]);
    }

    #[tokio::test]
    async fn test_mixed_prod_and_dev_deps_cycle_falls_back() {
        // A prod→B, B dev→A — forms a cycle with all edges,
        // falls back to prod-only where only A→B remains
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"A","dependencies":{"B":"*"}}"#,
            r#"{"name":"B","devDependencies":{"A":"*"}}"#,
        );
        let result = WorkspaceService::build_workspace_topology(temp.path()).await;
        assert!(result.is_ok(), "mixed prod+dev cycle should fall back");
        let topo = result.unwrap();
        assert_eq!(topo.edges.len(), 2);
        // Fallback ordering: only prod edge (A depends on B) → B first
        assert_eq!(topo.topology.len(), 2);
    }

    #[tokio::test]
    async fn test_build_workspace_json_edges_order() {
        let temp = tempdir().unwrap();
        create_mock_workspace(temp.path());
        let result = WorkspaceService::build_workspace_json(temp.path()).await;
        println!("{result:?}");
        assert!(result.is_ok(), "build_workspace_json should succeed");
        let json = result.unwrap();
        // Edges should be [["A", "B"]], meaning B depends on A
        assert_eq!(json.edges.len(), 1);
        assert_eq!(json.edges[0], ["A".to_string(), "B".to_string()]);
    }

    #[tokio::test]
    async fn test_duplicate_workspace_names_fail_with_paths() {
        let temp = tempdir().unwrap();
        create_mock_workspace_with(
            temp.path(),
            r#"{"name":"duplicate"}"#,
            r#"{"name":"duplicate"}"#,
        );

        let error = WorkspaceService::build_workspace_topology(temp.path())
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("EDUPLICATEWORKSPACE"));
        assert!(error.contains("`duplicate`"));
        assert!(error.contains("A/package.json"));
        assert!(error.contains("B/package.json"));
    }

    #[test]
    fn test_workspace_filter_from_flags() {
        assert!(matches!(
            WorkspaceFilter::from_flags(vec![], false),
            WorkspaceFilter::Current
        ));
        assert!(matches!(
            WorkspaceFilter::from_flags(vec![], true),
            WorkspaceFilter::All
        ));
        // --workspaces wins over --workspace
        assert!(matches!(
            WorkspaceFilter::from_flags(vec!["a".into()], true),
            WorkspaceFilter::All
        ));
        assert!(matches!(
            WorkspaceFilter::from_flags(vec!["a".into()], false),
            WorkspaceFilter::Selected(_)
        ));
    }

    #[test]
    fn test_project_topology_layers() {
        let topology = vec![
            vec!["a".into(), "b".into()],
            vec!["c".into(), "d".into()],
            vec!["e".into()],
        ];
        let mut selected = BTreeSet::new();
        selected.insert("a".into());
        selected.insert("d".into());
        let projected = project_topology_layers(&topology, &selected);
        // Layer 1 keeps only "a", layer 2 only "d", layer 3 is dropped.
        assert_eq!(
            projected,
            vec![vec!["a".to_string()], vec!["d".to_string()]]
        );
    }

    #[test]
    fn test_project_topology_layers_drops_empty() {
        let topology = vec![
            vec!["a".into()],
            vec!["b".into(), "c".into()],
            vec!["d".into()],
        ];
        let mut selected = BTreeSet::new();
        selected.insert("c".into());
        let projected = project_topology_layers(&topology, &selected);
        // Only layer 2's "c" survives; layers 1 and 3 are dropped entirely.
        assert_eq!(projected, vec![vec!["c".to_string()]]);
    }

    #[test]
    fn test_expand_filters_against_nodes() {
        let nodes = vec![
            WorkspaceNode {
                name: "@scope/a".into(),
                path: PathBuf::from("packages/a"),
            },
            WorkspaceNode {
                name: "@scope/b".into(),
                path: PathBuf::from("packages/b"),
            },
        ];

        // Match by name
        let by_name = expand_filters(&nodes, &["@scope/a".into()]).unwrap();
        assert!(by_name.contains("@scope/a"));
        assert_eq!(by_name.len(), 1);

        // Match by relative path
        let by_path = expand_filters(&nodes, &["packages/b".into()]).unwrap();
        assert!(by_path.contains("@scope/b"));
        assert_eq!(by_path.len(), 1);

        // Glob match
        let by_glob = expand_filters(&nodes, &["packages/*".into()]).unwrap();
        assert_eq!(by_glob.len(), 2);
        assert!(by_glob.contains("@scope/a"));
        assert!(by_glob.contains("@scope/b"));

        // Multiple filters dedupe
        let multi = expand_filters(&nodes, &["@scope/a".into(), "packages/a".into()]).unwrap();
        assert_eq!(multi.len(), 1);

        // No-match name errors
        assert!(expand_filters(&nodes, &["does-not-exist".into()]).is_err());
    }

    #[tokio::test]
    async fn test_resolve_layers_preserves_topology() {
        // B depends on A → expected layers: [[A], [B]]
        let dir = tempdir().unwrap();
        create_mock_workspace(dir.path());
        let root = dir.path();

        // Selecting both should preserve the layered topology
        let resolved = WorkspaceService::resolve_layers(
            root,
            WorkspaceFilter::Selected(vec!["A".into(), "B".into()]),
        )
        .await
        .unwrap();
        match resolved {
            ResolvedWorkspaces::Layers { layers, paths } => {
                assert_eq!(layers, vec![vec!["A".to_string()], vec!["B".to_string()]]);
                assert!(paths.contains_key("A"));
                assert!(paths.contains_key("B"));
            }
            _ => panic!("expected layered result"),
        }

        // Selecting only B should yield a single layer with just B
        let resolved =
            WorkspaceService::resolve_layers(root, WorkspaceFilter::Selected(vec!["B".into()]))
                .await
                .unwrap();
        match resolved {
            ResolvedWorkspaces::Layers { layers, .. } => {
                assert_eq!(layers, vec![vec!["B".to_string()]]);
            }
            _ => panic!("expected layered result"),
        }

        // Current filter -> Current outcome
        let resolved = WorkspaceService::resolve_layers(root, WorkspaceFilter::Current)
            .await
            .unwrap();
        assert!(matches!(resolved, ResolvedWorkspaces::Current));

        // All filter -> full topology layers
        let resolved = WorkspaceService::resolve_layers(root, WorkspaceFilter::All)
            .await
            .unwrap();
        match resolved {
            ResolvedWorkspaces::Layers { layers, .. } => {
                assert_eq!(layers, vec![vec!["A".to_string()], vec!["B".to_string()]]);
            }
            _ => panic!("expected layered result"),
        }
    }
}
