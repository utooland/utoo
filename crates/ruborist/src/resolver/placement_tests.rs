use std::borrow::Cow;

use super::*;
use crate::model::graph::{DependencyLookup, GraphEdge, PackageNode};
use crate::model::manifest::CoreVersionManifest;
use crate::model::node::EdgeType;
use crate::model::package_json::PackageJson;

fn manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
    let mut manifest = CoreVersionManifest {
        name: name.to_string(),
        version: version.to_string(),
        ..Default::default()
    };
    manifest.dist.tarball = Some(format!("https://registry.example.com/{name}-{version}.tgz"));
    Arc::new(manifest)
}

fn add_package(
    graph: &mut DependencyGraph,
    parent: NodeIndex,
    name: &str,
    version: &str,
) -> NodeIndex {
    let path = graph.graph[parent].path.join("node_modules").join(name);
    let index = graph.add_node(PackageNode::from_version_manifest(
        name.to_string(),
        path,
        manifest(name, version),
    ));
    graph.add_physical_edge(parent, index);
    index
}

fn add_shared_edge(graph: &mut DependencyGraph, from: NodeIndex, spec: &str) -> DependencyEdgeInfo {
    DependencyEdgeInfo {
        edge_id: graph.add_dependency_edge(from, "shared", spec, EdgeType::Prod),
        name: "shared".to_string(),
        spec: spec.to_string(),
        edge_type: EdgeType::Prod,
    }
}

fn assert_edge_target(graph: &DependencyGraph, edge: &DependencyEdgeInfo, target: NodeIndex) {
    let GraphEdge::Dependency(dependency) = &graph.graph[edge.edge_id] else {
        panic!("expected a dependency edge");
    };
    assert!(dependency.valid);
    assert_eq!(dependency.to, Some(target));
}

#[test]
fn invalidated_edges_require_resolution_before_reusing_a_matching_target() {
    let pkg = PackageJson::from_value(&serde_json::json!({
        "name": "root", "version": "1.0.0",
        "overrides": { "shared": "1.0.0" }
    }))
    .unwrap();
    let mut graph = DependencyGraph::from_package_json(".".into(), pkg);
    let root = graph.root_index;
    let shared = add_package(&mut graph, root, "shared", "1.0.0");
    let edge = add_shared_edge(&mut graph, root, "^1.0.0");
    graph.mark_dependency_resolved(edge.edge_id, shared);
    graph.invalidate_dependency(edge.edge_id);

    // A compatible locked node cannot settle an invalidated requirement until
    // its new override inputs have been resolved.
    assert!(graph.requires_resolution(edge.edge_id));
    assert!(try_reuse_dependency(&mut graph, root, &edge).is_none());

    let resolved = ResolvedDependency {
        spec: Cow::Borrowed("1.0.0"),
        manifest: manifest("shared", "1.0.0"),
    };
    let result = place_resolved_dependency(
        &mut graph,
        root,
        &edge,
        &resolved,
        &BuildDepsConfig::default(),
    );
    assert!(matches!(result, ProcessResult::Reused(index) if index == shared));
    assert!(!graph.requires_resolution(edge.edge_id));
    assert_eq!(graph.graph.node_count(), 2);
    assert_edge_target(&graph, &edge, shared);
}

#[test]
fn placement_keeps_scoped_override_in_requester_context() {
    let pkg = PackageJson::from_value(&serde_json::json!({
        "name": "root", "version": "1.0.0",
        "overrides": { "consumer": { "shared@^1.0.0": "2.0.0" } }
    }))
    .unwrap();
    let mut graph = DependencyGraph::from_package_json(".".into(), pkg);
    let root = graph.root_index;
    let consumer = add_package(&mut graph, root, "consumer", "1.0.0");
    let consumer_edge = graph.add_dependency_edge(root, "consumer", "1.0.0", EdgeType::Prod);
    graph.mark_dependency_resolved(consumer_edge, consumer);
    let shared = add_package(&mut graph, root, "shared", "1.0.0");
    let edge = add_shared_edge(&mut graph, consumer, "^1.0.0");
    graph.mark_dependency_resolved(edge.edge_id, shared);
    graph.invalidate_dependency(edge.edge_id);

    let resolved = ResolvedDependency {
        spec: Cow::Borrowed("2.0.0"),
        manifest: manifest("shared", "2.0.0"),
    };
    let result = place_resolved_dependency(
        &mut graph,
        consumer,
        &edge,
        &resolved,
        &BuildDepsConfig::default(),
    );
    let ProcessResult::Created(created) = result else {
        panic!("the scoped override needs a replacement in the consumer's context");
    };
    assert_eq!(graph.get_physical_parent(created), Some(consumer));
    assert_eq!(graph.graph[created].version, "2.0.0");
    assert!(!graph.requires_resolution(edge.edge_id));
    assert_edge_target(&graph, &edge, created);
}

#[test]
fn placement_preserves_compatible_range_reuse() {
    let mut graph =
        DependencyGraph::from_package_json(".".into(), PackageJson::new("root", "1.0.0"));
    let root = graph.root_index;
    let shared = add_package(&mut graph, root, "shared", "1.0.0");
    let edge = add_shared_edge(&mut graph, root, "^1.0.0");
    let resolved = ResolvedDependency {
        spec: Cow::Borrowed("^1.0.0"),
        manifest: manifest("shared", "1.1.0"),
    };
    let result = place_resolved_dependency(
        &mut graph,
        root,
        &edge,
        &resolved,
        &BuildDepsConfig::default(),
    );
    assert!(matches!(result, ProcessResult::Reused(index) if index == shared));
    assert_eq!(graph.graph.node_count(), 2);
    assert_edge_target(&graph, &edge, shared);
}

#[test]
fn placement_uses_final_override_target_instead_of_original_range() {
    for final_spec in ["2.0.0", "latest"] {
        let pkg = PackageJson::from_value(&serde_json::json!({
            "name": "root", "version": "1.0.0",
            "overrides": { "shared@^1.5.0": final_spec }
        }))
        .unwrap();
        let mut graph = DependencyGraph::from_package_json(".".into(), pkg);
        let root = graph.root_index;
        let consumer = add_package(&mut graph, root, "consumer", "1.0.0");
        let edge = add_shared_edge(&mut graph, consumer, "^1.0.0");
        // Resolution selected an override for 1.5.x; an older candidate was
        // placed meanwhile and satisfies only the original request.
        let resolved = ResolvedDependency {
            spec: Cow::Borrowed(final_spec),
            manifest: manifest("shared", "2.0.0"),
        };
        let stale = add_package(&mut graph, root, "shared", "1.0.0");
        let result = place_resolved_dependency(
            &mut graph,
            consumer,
            &edge,
            &resolved,
            &BuildDepsConfig::default(),
        );
        let ProcessResult::Created(created) = result else {
            panic!("{final_spec} must not reuse the stale original-range candidate");
        };
        assert_ne!(created, stale);
        assert_eq!(graph.graph[created].version, "2.0.0");
        assert_eq!(graph.get_physical_parent(created), Some(consumer));
        assert_edge_target(&graph, &edge, created);
    }
}

#[test]
fn placement_reuses_final_range_without_applying_overrides_again() {
    let pkg = PackageJson::from_value(&serde_json::json!({
        "name": "root", "version": "1.0.0",
        "overrides": {
            "shared@^1.0.0": "^2.0.0",
            "shared@^2.0.0": "3.0.0"
        }
    }))
    .unwrap();
    let mut graph = DependencyGraph::from_package_json(".".into(), pkg);
    let root = graph.root_index;
    let edge = add_shared_edge(&mut graph, root, "^1.0.0");
    let resolved = ResolvedDependency {
        spec: Cow::Borrowed("^2.0.0"),
        manifest: manifest("shared", "2.5.0"),
    };
    let shared = add_package(&mut graph, root, "shared", "2.1.0");
    let result = place_resolved_dependency(
        &mut graph,
        root,
        &edge,
        &resolved,
        &BuildDepsConfig::default(),
    );
    assert!(matches!(result, ProcessResult::Reused(index) if index == shared));
    assert_eq!(graph.graph.node_count(), 2);
    assert_eq!(graph.get_physical_parent(shared), Some(root));
    assert_edge_target(&graph, &edge, shared);
}

#[test]
fn placement_respects_nearer_candidate_shadowing_after_resolution() {
    for final_spec in ["2.0.0", "latest"] {
        let mut graph =
            DependencyGraph::from_package_json(".".into(), PackageJson::new("root", "1.0.0"));
        let root = graph.root_index;
        let ancestor = add_package(&mut graph, root, "shared", "2.0.0");
        let consumer = add_package(&mut graph, root, "consumer", "1.0.0");
        let nested = add_package(&mut graph, consumer, "shared", "1.0.0");
        let edge = add_shared_edge(&mut graph, consumer, final_spec);
        let resolved = ResolvedDependency {
            spec: Cow::Borrowed(final_spec),
            manifest: manifest("shared", "2.0.0"),
        };
        let result = place_resolved_dependency(
            &mut graph,
            consumer,
            &edge,
            &resolved,
            &BuildDepsConfig::default(),
        );
        let ProcessResult::Created(created) = result else {
            panic!("{final_spec} must not reuse a shadowed ancestor");
        };
        assert_ne!(created, ancestor);
        assert_ne!(created, nested);
        assert_eq!(graph.graph[created].version, "2.0.0");
        assert_eq!(graph.get_physical_parent(created), Some(consumer));
        assert_eq!(
            graph.lookup_dependency(consumer, "shared"),
            DependencyLookup::Found(created)
        );
        assert_edge_target(&graph, &edge, created);
    }
}
