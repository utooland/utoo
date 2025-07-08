use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Mutex;
use tokio::sync::Semaphore;

use crate::helper::install_runtime::install_runtime;
use crate::helper::workspace::find_workspaces;
use crate::util::config::get_legacy_peer_deps;
use crate::util::json::load_package_json_from_path;
use crate::util::logger::{
    PROGRESS_BAR, finish_progress_bar, log_progress, log_verbose, start_progress_bar,
};
use crate::util::node::{Edge, EdgeType, Node, get_node_from_root_by_path};
use crate::util::registry::{ResolvedPackage, load_cache, resolve_dependency, store_cache};
use crate::util::semver::matches;

pub struct Ruborist {
    path: PathBuf,
    pub ideal_tree: Option<Arc<Node>>,
}

use once_cell::sync::Lazy;

// concurrency limit default to 100
static CONCURRENCY_LIMITER: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(100)));

pub async fn build_deps(root: Arc<Node>) -> Result<()> {
    let legacy_peer_deps = get_legacy_peer_deps();
    log_verbose(&format!(
        "going to build deps for {root}, legacy_peer_deps: {legacy_peer_deps}"
    ));
    let current_level = Arc::new(Mutex::new(vec![root.clone()]));
    // Track processed workspace nodes to prevent cycles
    let processed_workspace_nodes = Arc::new(Mutex::new(std::collections::HashSet::new()));

    while !current_level.lock().unwrap().is_empty() {
        let next_level = Arc::new(Mutex::new(Vec::new()));
        let nodes = current_level.lock().unwrap().clone();
        let mut level_tasks = Vec::new();

        for node in nodes {
            let edges = node.edges_out.read().unwrap();
            let total_deps = edges.len();
            PROGRESS_BAR.inc_length(total_deps as u64);
            log_progress(&format!("Resolving dependencies for {}", node.name));

            let mut tasks = Vec::new();

            for edge in edges.iter() {
                let edge = edge.clone();
                let next_level = next_level.clone();
                let processed_workspace_nodes = processed_workspace_nodes.clone();

                tasks.push(async move {
                    let _permit = CONCURRENCY_LIMITER.acquire().await.unwrap();

                    if *edge.valid.read().unwrap() {
                        log_verbose(&format!("deps {}@{} already resolved", edge.name, edge.spec));

                        // Only process workspace nodes from root to avoid cycles
                        if !edge.from.is_root {
                            return Ok(());
                        }

                        // Add workspace node to next level if not processed before
                        if let Some(new_node) = edge.to.read().unwrap().as_ref().cloned() {
                            if !new_node.is_workspace {
                                return Ok(());
                            }

                            let mut processed = processed_workspace_nodes.lock().unwrap();
                            if !processed.contains(&new_node.name) {
                                processed.insert(new_node.name.clone());
                                next_level.lock().unwrap().push(new_node);
                            }
                        }

                        return Ok(());
                    }

                    log_verbose(&format!("going to build deps {}@{} from [{}]", edge.name, edge.spec, edge.from));

                    match find_compatible_node(&edge.from, &edge.name, &edge.spec) {
                        FindResult::Reuse(existing_node) => {
                            log_verbose(&format!(
                                "resolved deps {}@{} => {} (reuse)",
                                edge.name, &edge.spec, existing_node.version
                            ));
                            {
                                let mut to = edge.to.write().unwrap();
                                *to = Some(existing_node.clone());
                                let mut valid = edge.valid.write().unwrap();
                                *valid = true;
                            }

                            // update node type by edges
                            existing_node.add_invoke(&edge);
                            existing_node.update_type();
                        }
                        FindResult::Conflict(conflict_node) => {
                            let resolved = match resolve_dependency(&edge.name, &edge.spec, &edge.edge_type).await? {
                                Some(resolved) => resolved,
                                None => return Ok(()),
                            };
                            PROGRESS_BAR.inc(1);
                            log_progress(&format!(
                                "resolved deps {}@{} => {} (conflict), need to fork, conflict_node: {}",
                                edge.name, &edge.spec, resolved.version, conflict_node
                            ));
                            log_verbose(&format!(
                                "resolved deps {}@{} => {} (conflict), need to fork, conflict_node: {}",
                                edge.name, &edge.spec, resolved.version, conflict_node
                            ));
                            // process conflict node
                            let install_parent = conflict_node;
                            let new_node = place_deps(edge.name.clone(), resolved, &install_parent)
                                .with_context(|| format!("Failed to place dependencies for {}@{} in conflict case", edge.name, edge.spec))?;


                            {
                                let mut parent = new_node.parent.write().unwrap();
                                *parent = Some(install_parent.clone());
                                let mut children = install_parent.children.write().unwrap();
                                children.push(new_node.clone());


                                let mut to = edge.to.write().unwrap();
                                *to = Some(new_node.clone());
                                let mut valid = edge.valid.write().unwrap();
                                *valid = true;
                                // update node type
                                new_node.add_invoke(&edge);
                                new_node.update_type();
                            }

                            let dep_types = if legacy_peer_deps {
                                vec![
                                    ("dependencies", EdgeType::Prod),
                                    ("optionalDependencies", EdgeType::Optional),
                                ]
                            } else {
                                vec![
                                    ("dependencies", EdgeType::Prod),
                                    ("peerDependencies", EdgeType::Peer),
                                    ("optionalDependencies", EdgeType::Optional),
                                ]
                            };

                            for (field, edge_type) in dep_types {
                                if let Some(deps) = new_node.package.get(field)
                                    && let Some(deps) = deps.as_object() {
                                        for (name, version) in deps {
                                            let version_spec = version.as_str().unwrap_or("").to_string();
                                            let dep_edge = Edge::new(new_node.clone(), edge_type.clone(), name.clone(), version_spec);
                                            log_verbose(&format!(
                                                "add edge {}@{} for {}",
                                                name, version, new_node.name
                                            ));
                                            new_node.add_edge(dep_edge).await;
                                        }
                                    }
                            }

                            next_level.lock().unwrap().push(new_node);
                        }
                        FindResult::New(install_location) => {
                            let resolved = match resolve_dependency(&edge.name, &edge.spec, &edge.edge_type).await? {
                                Some(resolved) => resolved,
                                None => return Ok(()),
                            };
                            PROGRESS_BAR.inc(1);
                            log_progress(&format!(
                                "resolved deps {}@{} => {} (new)",
                                edge.name, &edge.spec, resolved.version
                            ));
                            log_verbose(&format!(
                                "resolved deps {}@{} => {} (new)",
                                edge.name, &edge.spec, resolved.version
                            ));
                            let new_node = place_deps(edge.name.clone(), resolved, &install_location)
                                .with_context(|| format!("Failed to place dependencies for {}@{} in new case", edge.name, edge.spec))?;
                            let root_node = install_location.clone();

                            {
                                let mut parent = new_node.parent.write().unwrap();
                                *parent = Some(root_node.clone());
                            }
                            {
                                let mut children = root_node.children.write().unwrap();
                                children.push(new_node.clone());
                            }
                            {
                                let mut to = edge.to.write().unwrap();
                                *to = Some(new_node.clone());
                                let mut valid = edge.valid.write().unwrap();
                                *valid = true;
                                // update node type
                                new_node.add_invoke(&edge);
                                new_node.update_type();
                            }

                            add_dependency_edge(&new_node, "dependencies", EdgeType::Prod).await;

                            if !legacy_peer_deps {
                                add_dependency_edge(&new_node, "peerDependencies", EdgeType::Peer).await;
                            }

                            add_dependency_edge(&new_node, "optionalDependencies", EdgeType::Optional).await;

                            next_level.lock().unwrap().push(new_node);
                        }
                    }
                    Ok::<_, anyhow::Error>(())
                });
            }
            level_tasks.push(futures::future::try_join_all(tasks));
        }

        // waiting for all tasks in this level to finish
        futures::future::try_join_all(level_tasks)
            .await
            .map_err(|e| {
                let err_msg = e
                    .chain()
                    .map(|err| format!("  {err}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                anyhow::anyhow!(err_msg)
            })?;

        // continue to next level
        *current_level.lock().unwrap() = next_level.lock().unwrap().clone();
    }

    Ok(())
}

// create a new node under parent
fn place_deps(name: String, pkg: ResolvedPackage, parent: &Arc<Node>) -> Result<Arc<Node>> {
    let new_node = Node::new(name, parent.path.clone(), pkg.manifest);

    log_verbose(&format!(
        "\nInstalling {}@{} under parent chain: {}",
        new_node.name, new_node.version, parent
    ));
    // log_verbose(&print_parent_chain(parent));
    log_verbose("");

    Ok(new_node)
}

#[derive(Debug)]
pub enum FindResult {
    Reuse(Arc<Node>),    // can resue
    Conflict(Arc<Node>), // conflict, return parent node
    New(Arc<Node>),      // need to install under root node
}

fn find_compatible_node(from: &Arc<Node>, name: &str, version_spec: &str) -> FindResult {
    fn find_in_node(
        node: &Arc<Node>,
        name: &str,
        version_spec: &str,
        current: &Arc<Node>,
    ) -> FindResult {
        let children = node.children.read().unwrap();

        for child in children.iter() {
            if child.name == name {
                if matches(version_spec, &child.version) {
                    log_verbose(&format!(
                        "found existing deps {}@{} got {}, place {}",
                        name, version_spec, child.version, child
                    ));
                    return FindResult::Reuse(child.clone());
                } else {
                    log_verbose(&format!(
                        "found conflict deps {}@{} got {}, place {}",
                        name, version_spec, child.version, child
                    ));
                    return FindResult::Conflict(current.clone());
                }
            }
        }

        // find in parent
        if let Some(parent) = node.parent.read().unwrap().as_ref() {
            find_in_node(parent, name, version_spec, current)
        } else {
            // not found, return new
            FindResult::New(node.clone())
        }
    }

    if let Some(parent) = from.parent.read().unwrap().as_ref() {
        find_in_node(parent, name, version_spec, from)
    } else {
        find_in_node(from, name, version_spec, from)
    }
}

impl Ruborist {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
            ideal_tree: None,
        }
    }

    async fn init_runtime(&mut self, root: Arc<Node>) -> Result<()> {
        let deps = install_runtime(root.package.get("engines").unwrap_or(&Value::Null))?;
        for (name, version) in deps {
            let edge = Edge::new(root.clone(), EdgeType::Optional, name, version);
            root.add_edge(edge).await;
        }
        Ok(())
    }

    async fn init_tree(&mut self) -> Result<Arc<Node>> {
        // load package.json
        let pkg = load_package_json_from_path(&self.path)?;

        // create root node
        let root = Node::new_root(
            pkg["name"].as_str().unwrap_or("root").to_string(),
            self.path.clone(),
            pkg.clone(),
        );
        log_verbose(&format!("root node: {root:?}"));

        self.init_runtime(root.clone()).await?;
        self.init_workspaces(root.clone()).await?;

        // collect deps type
        let legacy_peer_deps = get_legacy_peer_deps();
        let dep_types = if legacy_peer_deps {
            vec![
                ("dependencies", EdgeType::Prod),
                ("devDependencies", EdgeType::Dev),
                ("optionalDependencies", EdgeType::Optional),
            ]
        } else {
            vec![
                ("dependencies", EdgeType::Prod),
                ("devDependencies", EdgeType::Dev),
                ("peerDependencies", EdgeType::Peer),
                ("optionalDependencies", EdgeType::Optional),
            ]
        };

        // process deps in root
        for (field, dep_type) in dep_types {
            if let Some(deps) = pkg.get(field)
                && let Some(deps) = deps.as_object()
            {
                for (name, version) in deps {
                    log_verbose(&format!("{name}: {version}"));
                    let version_spec = version.as_str().unwrap_or("").to_string();

                    // create edge
                    let edge = Edge::new(
                        root.clone(), // need clone Arc<Node>
                        dep_type.clone(),
                        name.clone(),
                        version_spec,
                    );

                    log_verbose(&format!("add edge {}@{}", edge.name, edge.spec));
                    root.add_edge(edge).await;
                }
            }
        }

        Ok(root)
    }

    pub async fn init_workspaces(&mut self, root: Arc<Node>) -> Result<()> {
        let workspaces = find_workspaces(&self.path).await.map_err(|e| {
            let err_msg = e
                .chain()
                .map(|err| format!("  {err}"))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::anyhow!(err_msg)
        })?;

        // Process each workspace member
        for (name, path, pkg) in workspaces {
            let version = pkg["version"].as_str().unwrap_or("").to_string();

            // Create workspace node
            let workspace_node = Node::new_workspace(name.clone(), path, pkg.clone());

            // Create link node
            let link_node = Node::new_link(name.clone(), workspace_node.clone());

            // Create dependency edge
            let dep_edge = Edge::new(root.clone(), EdgeType::Prod, name.clone(), version);

            // Set target node and validity for dependency edge
            {
                let mut valid = dep_edge.valid.write().unwrap();
                *valid = true;

                let mut to = dep_edge.to.write().unwrap();
                *to = Some(workspace_node.clone());
            }

            // Update parent relationships
            {
                let mut parent = workspace_node.parent.write().unwrap();
                *parent = Some(root.clone());
            }
            {
                let mut parent = link_node.parent.write().unwrap();
                *parent = Some(root.clone());
            }
            {
                let mut children = root.children.write().unwrap();
                children.push(workspace_node.clone());
                children.push(link_node);
            }

            // Add dependency edge
            root.add_edge(dep_edge).await;

            log_verbose(&format!(
                "Added workspace: {} {:?}",
                name, workspace_node.path
            ));

            // Process workspace dependencies
            let legacy_peer_deps = get_legacy_peer_deps();
            let dep_types = if legacy_peer_deps {
                vec![
                    ("devDependencies", EdgeType::Dev),
                    ("dependencies", EdgeType::Prod),
                    ("optionalDependencies", EdgeType::Optional),
                ]
            } else {
                vec![
                    ("devDependencies", EdgeType::Dev),
                    ("dependencies", EdgeType::Prod),
                    ("peerDependencies", EdgeType::Peer),
                    ("optionalDependencies", EdgeType::Optional),
                ]
            };

            for (field, edge_type) in dep_types {
                if let Some(deps) = workspace_node.package.get(field)
                    && let Some(deps) = deps.as_object()
                {
                    for (name, version) in deps {
                        let version_spec = version.as_str().unwrap_or("").to_string();
                        let dep_edge = Edge::new(
                            workspace_node.clone(),
                            edge_type.clone(),
                            name.clone(),
                            version_spec,
                        );
                        log_verbose(&format!(
                            "add edge {}@{} for {}",
                            name, version, workspace_node.name
                        ));
                        workspace_node.add_edge(dep_edge).await;
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn build_ideal_tree(&mut self) -> Result<()> {
        let cache_path = self.path.join("./node_modules/.utoo-manifest.json");
        load_cache(&cache_path)
            .await
            .context("Failed to load cache")?;
        let root = self.init_tree().await?;

        start_progress_bar();
        build_deps(root.clone()).await?;

        let res = self.get_dup_deps(root.clone());
        for dup_node in res {
            self.replace_deps(dup_node).await?;
        }

        store_cache(&cache_path)
            .await
            .context("Failed to store cache")?;
        finish_progress_bar("Building dependency tree complete.");
        self.ideal_tree = Some(root);
        Ok(())
    }

    pub async fn build_workspace_tree(&mut self) -> Result<()> {
        let root = self.init_tree().await?;

        start_progress_bar();

        // init_tree has already loaded all workspace
        let children = root.children.read().unwrap();

        // build a map of workspace nodes
        let mut workspace_map = HashMap::new();
        for workspace in children.iter() {
            if workspace.is_workspace {
                workspace_map.insert(workspace.name.clone(), workspace.clone());
            }
        }

        // find the deps between workspace
        for workspace in children.iter() {
            if workspace.is_link {
                continue;
            }
            PROGRESS_BAR.inc_length(1);
            let edges = workspace.edges_out.read().unwrap();
            for edge in edges.iter() {
                if let Some(dep_workspace) = workspace_map.get(&edge.name) {
                    // find edges_out for workspace
                    let mut to = edge.to.write().unwrap();
                    *to = Some(dep_workspace.clone());
                    let mut valid = edge.valid.write().unwrap();
                    *valid = true;

                    log_verbose(&format!(
                        "Workspace dependency: {} -> {}",
                        workspace.name, dep_workspace.name
                    ));
                }
            }
            PROGRESS_BAR.inc(1);
        }

        finish_progress_bar("Building workspace dependency tree complete.");
        self.ideal_tree = Some(root.clone());
        Ok(())
    }

    pub fn get_dup_deps(&self, root: Arc<Node>) -> Vec<Arc<Node>> {
        let mut duplicates = Vec::new();

        fn process_node(node: &Arc<Node>, duplicates: &mut Vec<Arc<Node>>) {
            let children = node.children.read().unwrap();
            let mut name_map: HashMap<String, Vec<Arc<Node>>> = HashMap::new();

            // find duplicate deps
            for child in children.iter() {
                if child.is_workspace {
                    continue;
                }
                name_map
                    .entry(child.name.clone())
                    .or_default()
                    .push(child.clone());
            }

            // hanlde dup node
            for (_, nodes) in name_map {
                if nodes.len() > 1 {
                    // find max edges_in node to save the cost
                    let mut max_edges = 0;
                    let mut primary_node = None;

                    for node in &nodes {
                        let edges_count = node.edges_in.read().unwrap().len();
                        if edges_count > max_edges {
                            max_edges = edges_count;
                            primary_node = Some(node.clone());
                        }
                    }

                    // add to duplicates
                    if let Some(primary) = primary_node {
                        for node in nodes {
                            if !Arc::ptr_eq(&node, &primary) {
                                duplicates.push(node);
                            }
                        }
                    }
                }
            }

            for child in children.iter() {
                process_node(child, duplicates);
            }
        }

        process_node(&root, &mut duplicates);
        duplicates
    }

    pub async fn replace_deps(&self, node: Arc<Node>) -> Result<()> {
        log_verbose(&format!("going to replace node {node}"));
        // 1. remove from parent node
        if let Some(parent) = node.parent.read().unwrap().as_ref() {
            let mut parent_children = parent.children.write().unwrap();
            parent_children.retain(|child| !Arc::ptr_eq(child, &node));
        }

        // 2. clean edges_out
        {
            let mut edges_out = node.edges_out.write().unwrap();
            edges_out.clear();
        }

        // 3. clean edges_in
        let edges_from = {
            let edges_in = node.edges_in.read().unwrap();
            edges_in
                .iter()
                .map(|edge| {
                    let mut valid = edge.valid.write().unwrap();
                    *valid = false;

                    let mut to = edge.to.write().unwrap();
                    *to = None;

                    edge.from.clone()
                })
                .collect::<Vec<_>>()
        };

        // 4. rebuild deps
        for from_node in edges_from {
            build_deps(from_node).await?;
        }

        Ok(())
    }

    pub async fn fix_dep_path(&self, pkg_path: &str, pkg_name: &str) -> Result<()> {
        let root = self
            .ideal_tree
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Ideal tree not initialized"))?;

        let current_node = get_node_from_root_by_path(root, pkg_path).await?;

        // Now we have the target node, find and fix the dependency
        let edges_to_fix = {
            let edges = current_node.edges_out.read().unwrap();
            edges
                .iter()
                .filter(|edge| edge.name == pkg_name)
                .cloned()
                .collect::<Vec<_>>()
        };

        for edge in edges_to_fix {
            let to_node = {
                let to_guard = edge.to.read().unwrap();
                to_guard.as_ref().unwrap().clone()
            };
            log_verbose(&format!(
                "Fixing dependency: {}, from: {}, to: {}",
                edge.name, edge.from, to_node
            ));
            *edge.valid.write().unwrap() = false;
            build_deps(current_node.clone()).await?;
        }

        Ok(())
    }
}

async fn add_dependency_edge(node: &Arc<Node>, field: &str, edge_type: EdgeType) {
    if let Some(deps) = node.package.get(field)
        && let Some(deps) = deps.as_object()
    {
        for (name, version) in deps {
            let version_spec = version.as_str().unwrap_or("").to_string();
            let dep_edge = Edge::new(node.clone(), edge_type.clone(), name.clone(), version_spec);
            log_verbose(&format!("add edge {}@{} for {}", name, version, node.name));
            node.add_edge(dep_edge).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::path::PathBuf;

    use super::*;
    use crate::util::node::Node;

    #[tokio::test]
    async fn test_fix_dep_path() {
        // Create a mock root node
        let root = Node::new_root(
            "test-package".to_string(),
            PathBuf::from("."),
            json!({
                "name": "test-package",
                "version": "1.0.0",
                "dependencies": {
                    "lodash": "^4.17.20"
                }
            }),
        );

        // Create a child node
        let child = Node::new(
            "lodash".to_string(),
            PathBuf::from("node_modules/lodash"),
            json!({
                "name": "lodash",
                "version": "4.17.20"
            }),
        );

        // Add child to root
        {
            let mut children = root.children.write().unwrap();
            children.push(child.clone());
        }

        // Create Ruborist instance
        let mut ruborist = Ruborist::new(PathBuf::from("."));
        ruborist.ideal_tree = Some(root.clone());

        // Test fixing dependency path
        let result = ruborist.fix_dep_path("", "lodash").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fix_dep_path_with_invalid_path() {
        // Create a mock root node
        let root = Node::new_root(
            "test-package".to_string(),
            PathBuf::from("."),
            json!({
                "name": "test-package",
                "version": "1.0.0"
            }),
        );

        // Create Ruborist instance
        let mut ruborist = Ruborist::new(PathBuf::from("."));
        ruborist.ideal_tree = Some(root.clone());

        // Test fixing non-existent dependency path
        let result = ruborist.fix_dep_path("invalid/path", "lodash").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fix_dep_path_without_ideal_tree() {
        // Create Ruborist instance without ideal tree
        let ruborist = Ruborist::new(PathBuf::from("."));

        // Test fixing dependency path without ideal tree
        let result = ruborist.fix_dep_path("", "lodash").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_deps_with_workspace_cycle() {
        // Create a mock root node
        let root = Node::new_root(
            "test-monorepo".to_string(),
            PathBuf::from("."),
            json!({
                "name": "test-monorepo",
                "version": "1.0.0",
                "workspaces": ["packages/*"]
            }),
        );

        // Create workspace A
        let workspace_a = Node::new_workspace(
            "workspace-a".to_string(),
            PathBuf::from("packages/workspace-a"),
            json!({
                "name": "workspace-a",
                "version": "1.0.0",
                "dependencies": {
                    "workspace-b": "1.0.0"
                }
            }),
        );

        // Create workspace B
        let workspace_b = Node::new_workspace(
            "workspace-b".to_string(),
            PathBuf::from("packages/workspace-b"),
            json!({
                "name": "workspace-b",
                "version": "1.0.0",
                "dependencies": {
                    "workspace-a": "1.0.0"
                }
            }),
        );

        // Add workspaces to root
        {
            let mut children = root.children.write().unwrap();
            children.push(workspace_a.clone());
            children.push(workspace_b.clone());
        }

        // Create dependency edges
        let edge_a_to_b = Edge::new(
            workspace_a.clone(),
            EdgeType::Prod,
            "workspace-b".to_string(),
            "1.0.0".to_string(),
        );
        let edge_b_to_a = Edge::new(
            workspace_b.clone(),
            EdgeType::Prod,
            "workspace-a".to_string(),
            "1.0.0".to_string(),
        );

        // Add edges to workspaces
        workspace_a.add_edge(edge_a_to_b).await;
        workspace_b.add_edge(edge_b_to_a).await;

        // Test build_deps
        let result = build_deps(root.clone()).await;
        assert!(
            result.is_ok(),
            "build_deps should handle workspace cycles successfully"
        );

        // Verify that both workspaces are processed exactly once
        let mut processed_workspaces = std::collections::HashSet::new();
        let mut stack = vec![root];

        while let Some(node) = stack.pop() {
            let children = node.children.read().unwrap();
            for child in children.iter() {
                if child.is_workspace {
                    assert!(
                        !processed_workspaces.contains(&child.name),
                        "Workspace {} should only be processed once",
                        child.name
                    );
                    processed_workspaces.insert(child.name.clone());
                }
                stack.push(child.clone());
            }
        }

        assert_eq!(
            processed_workspaces.len(),
            2,
            "Should process exactly 2 workspaces"
        );
        assert!(
            processed_workspaces.contains("workspace-a"),
            "Should process workspace-a"
        );
        assert!(
            processed_workspaces.contains("workspace-b"),
            "Should process workspace-b"
        );
    }
}
