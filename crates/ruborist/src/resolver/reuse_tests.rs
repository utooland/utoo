//! Dependency reuse tests, including overrides and resolved identity.

use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::model::graph::PackageNode;
use crate::model::package_json::PackageJson;

fn create_pkg(name: &str, version: &str) -> PackageJson {
    PackageJson::new(name, version)
}

fn create_version_manifest(name: &str, version: &str) -> Arc<CoreVersionManifest> {
    Arc::new(CoreVersionManifest {
        name: name.to_string(),
        version: version.to_string(),
        ..Default::default()
    })
}

#[test]
fn ordinary_tags_keep_locked_candidates_but_override_tags_need_resolution() {
    for overrides in [
        serde_json::json!({}),
        serde_json::json!({ "shared": "latest" }),
    ] {
        let pkg = PackageJson::from_value(&serde_json::json!({
            "name": "root", "version": "1.0.0", "overrides": overrides
        }))
        .unwrap();
        let mut graph = DependencyGraph::from_package_json(".".into(), pkg);
        let shared = graph.add_node(PackageNode::from_version_manifest(
            "shared".into(),
            "node_modules/shared".into(),
            create_version_manifest("shared", "1.0.0"),
        ));
        graph.add_physical_edge(graph.root_index, shared);
        let result = find_reusable_node(&graph, graph.root_index, "shared", "latest");
        let expected = if overrides.as_object().unwrap().is_empty() {
            ReuseResult::Reuse(shared)
        } else {
            ReuseResult::Install(graph.root_index)
        };
        assert_eq!(result, expected);
    }
}

#[test]
fn test_find_reusable_node_reuse() {
    let pkg = create_pkg("root", "1.0.0");
    let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

    // Add lodash@4.17.21 under root
    let lodash = PackageNode::from_version_manifest(
        "lodash".to_string(),
        PathBuf::from("node_modules/lodash"),
        create_version_manifest("lodash", "4.17.21"),
    );
    let lodash_idx = graph.add_node(lodash);
    graph.add_physical_edge(graph.root_index, lodash_idx);

    // Should reuse existing lodash when spec matches
    let result = find_reusable_node(&graph, graph.root_index, "lodash", "^4.17.0");
    assert_eq!(result, ReuseResult::Reuse(lodash_idx));
}

#[test]
fn test_find_reusable_http_tarball() {
    for url in [
        "http://example.com/shared.tgz",
        "https://pkg.pr.new/shared@commit",
    ] {
        let other_url = format!("{url}?other");
        for (resolved, can_reuse) in [
            (Some(url), true),
            (Some(other_url.as_str()), false),
            (None, false),
        ] {
            let mut graph =
                DependencyGraph::from_package_json(PathBuf::from("."), create_pkg("root", "1.0.0"));
            let mut manifest = CoreVersionManifest {
                name: "shared".to_string(),
                version: "1.0.0".to_string(),
                ..Default::default()
            };
            manifest.dist.tarball = resolved.map(str::to_string);
            let shared = graph.add_node(PackageNode::from_version_manifest(
                "shared".to_string(),
                PathBuf::from("node_modules/shared"),
                Arc::new(manifest),
            ));
            graph.add_physical_edge(graph.root_index, shared);
            let consumer = graph.add_node(PackageNode::from_version_manifest(
                "consumer".to_string(),
                PathBuf::from("node_modules/consumer"),
                create_version_manifest("consumer", "1.0.0"),
            ));
            graph.add_physical_edge(graph.root_index, consumer);

            let expected = if can_reuse {
                ReuseResult::Reuse(shared)
            } else {
                ReuseResult::Install(consumer)
            };
            assert_eq!(
                find_reusable_node(&graph, consumer, "shared", url),
                expected,
                "requested {url}, resolved {resolved:?}"
            );
        }
    }
}

#[test]
fn test_find_reusable_node_conflict() {
    let pkg = create_pkg("root", "1.0.0");
    let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

    // Add lodash@3.10.1 under root
    let lodash = PackageNode::from_version_manifest(
        "lodash".to_string(),
        PathBuf::from("node_modules/lodash"),
        create_version_manifest("lodash", "3.10.1"),
    );
    let lodash_idx = graph.add_node(lodash);
    graph.add_physical_edge(graph.root_index, lodash_idx);

    // Should find conflict when spec doesn't match
    let result = find_reusable_node(&graph, graph.root_index, "lodash", "^4.17.0");
    assert_eq!(result, ReuseResult::Install(graph.root_index));
}

#[test]
fn test_conditional_override_matches_candidate_version_and_source() {
    let url = "https://registry.example.com/shared-1.0.0.tgz";
    for (target, manifest_name, resolved, can_reuse) in [
        ("1.0.0", "shared", Some(url), true),
        ("~1.0.0", "shared", Some(url), true),
        ("2.0.0", "shared", Some(url), false),
        ("npm:shared@1.0.0", "shared", Some(url), true),
        ("npm:patched@1.0.0", "shared", Some(url), false),
        ("npm:@scope/shared@1.0.0", "@scope/shared", Some(url), true),
        ("latest", "shared", Some(url), false),
        (url, "shared", Some(url), true),
        (
            "https://example.com/patched.tgz",
            "shared",
            Some(url),
            false,
        ),
        (url, "shared", None, false),
    ] {
        let pkg = PackageJson::from_value(&serde_json::json!({
            "name": "root", "version": "1.0.0",
            "overrides": { "shared@^1.0.0": target }
        }))
        .unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);
        let mut manifest = CoreVersionManifest {
            name: manifest_name.to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        manifest.dist.tarball = resolved.map(str::to_string);
        let shared = graph.add_node(PackageNode::from_version_manifest(
            "shared".to_string(),
            PathBuf::from("node_modules/shared"),
            Arc::new(manifest),
        ));
        graph.add_physical_edge(graph.root_index, shared);

        let expected = if can_reuse {
            ReuseResult::Reuse(shared)
        } else {
            ReuseResult::Install(graph.root_index)
        };
        for spec in ["^1.0.0", "latest"] {
            assert_eq!(
                find_reusable_node(&graph, graph.root_index, "shared", spec),
                expected,
                "request {spec}, override {target}, candidate {manifest_name}@1.0.0 from {resolved:?}",
            );
        }
    }
}

#[test]
fn test_find_resolved_node_matches_manifest_identity() {
    let url = "https://registry.example.com/shared-2.0.0.tgz";
    let mut resolved = CoreVersionManifest {
        name: "shared".to_string(),
        version: "2.0.0".to_string(),
        ..Default::default()
    };
    resolved.dist.tarball = Some(url.to_string());

    for (name, version, source, can_reuse) in [
        ("shared", "2.0.0", Some(url), true),
        ("other", "2.0.0", Some(url), false),
        ("shared", "1.0.0", Some(url), false),
        (
            "shared",
            "2.0.0",
            Some("https://example.com/patched.tgz"),
            false,
        ),
        ("shared", "2.0.0", None, false),
    ] {
        let pkg = PackageJson::from_value(&serde_json::json!({
            "name": "root", "version": "1.0.0",
            "overrides": { "shared@^1.0.0": "latest" }
        }))
        .unwrap();
        let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);
        let mut candidate = CoreVersionManifest {
            name: name.to_string(),
            version: version.to_string(),
            ..Default::default()
        };
        candidate.dist.tarball = source.map(str::to_string);
        let shared = graph.add_node(PackageNode::from_version_manifest(
            "shared".to_string(),
            PathBuf::from("node_modules/shared"),
            Arc::new(candidate),
        ));
        graph.add_physical_edge(graph.root_index, shared);

        assert_eq!(
            find_reusable_node(&graph, graph.root_index, "shared", "^1.0.0"),
            ReuseResult::Install(graph.root_index),
        );
        let expected = if can_reuse {
            ReuseResult::Reuse(shared)
        } else {
            ReuseResult::Install(graph.root_index)
        };
        assert_eq!(
            find_resolved_node(&graph, graph.root_index, "shared", "latest", &resolved),
            expected,
            "candidate {name}@{version} from {source:?}",
        );
    }
}

#[test]
fn test_find_reusable_node_new() {
    let pkg = create_pkg("root", "1.0.0");
    let graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

    // Should install at root when no existing node is found
    let result = find_reusable_node(&graph, graph.root_index, "lodash", "^4.17.0");
    assert_eq!(result, ReuseResult::Install(graph.root_index));
}

#[test]
fn test_find_reusable_node_nested() {
    let pkg = create_pkg("root", "1.0.0");
    let mut graph = DependencyGraph::from_package_json(PathBuf::from("."), pkg);

    // Add express under root
    let express = PackageNode::from_version_manifest(
        "express".to_string(),
        PathBuf::from("node_modules/express"),
        create_version_manifest("express", "4.18.0"),
    );
    let express_idx = graph.add_node(express);
    graph.add_physical_edge(graph.root_index, express_idx);

    // Add lodash@4.17.21 under root
    let lodash = PackageNode::from_version_manifest(
        "lodash".to_string(),
        PathBuf::from("node_modules/lodash"),
        create_version_manifest("lodash", "4.17.21"),
    );
    let lodash_idx = graph.add_node(lodash);
    graph.add_physical_edge(graph.root_index, lodash_idx);

    // From express, should find lodash in parent (root)
    let result = find_reusable_node(&graph, express_idx, "lodash", "^4.17.0");
    assert_eq!(result, ReuseResult::Reuse(lodash_idx));

    let nested_idx = graph.add_node(PackageNode::from_version_manifest(
        "lodash".to_string(),
        PathBuf::from("node_modules/express/node_modules/lodash"),
        create_version_manifest("lodash", "3.10.1"),
    ));
    graph.add_physical_edge(express_idx, nested_idx);

    // The nested copy wins even when an ancestor also satisfies the range.
    for spec in ["^3.0.0", "*"] {
        assert_eq!(
            find_reusable_node(&graph, express_idx, "lodash", spec),
            ReuseResult::Reuse(nested_idx)
        );
    }
    // An incompatible nested copy hides the otherwise compatible ancestor.
    assert_eq!(
        find_reusable_node(&graph, express_idx, "lodash", "^4.17.0"),
        ReuseResult::Install(express_idx)
    );
}

#[test]
fn test_find_resolved_node_uses_final_identity_for_file_and_git_sources() {
    for (spec, resolved_source, other_source) in [
        (
            "file:/project/vendor/shared.tgz",
            "file:/project/vendor/shared.tgz",
            "file:/project/vendor/patched-shared.tgz",
        ),
        (
            "git+https://github.com/utooland/shared.git",
            "git+https://github.com/utooland/shared.git#1111111111111111111111111111111111111111",
            "git+https://github.com/utooland/shared.git#2222222222222222222222222222222222222222",
        ),
        (
            "utooland/shared",
            "git+https://github.com/utooland/shared.git#1111111111111111111111111111111111111111",
            "git+https://github.com/utooland/shared.git#2222222222222222222222222222222222222222",
        ),
    ] {
        let mut resolved = CoreVersionManifest {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        resolved.dist.tarball = Some(resolved_source.to_string());

        // Git requests are unpinned; only the resolved source carries the
        // commit. Candidate identity must be checked against that final source.
        for (name, version, source, can_reuse) in [
            ("shared", "1.0.0", resolved_source, true),
            ("shared", "1.0.0", other_source, false),
            ("other", "1.0.0", resolved_source, false),
            ("shared", "2.0.0", resolved_source, false),
        ] {
            let mut graph =
                DependencyGraph::from_package_json(PathBuf::from("."), create_pkg("root", "1.0.0"));
            let mut candidate = CoreVersionManifest {
                name: name.to_string(),
                version: version.to_string(),
                ..Default::default()
            };
            candidate.dist.tarball = Some(source.to_string());
            let shared = graph.add_node(PackageNode::from_version_manifest(
                "shared".to_string(),
                PathBuf::from("node_modules/shared"),
                Arc::new(candidate),
            ));
            graph.add_physical_edge(graph.root_index, shared);
            let expected = if can_reuse {
                ReuseResult::Reuse(shared)
            } else {
                ReuseResult::Install(graph.root_index)
            };
            assert_eq!(
                find_resolved_node(&graph, graph.root_index, "shared", spec, &resolved),
                expected,
                "request {spec}, candidate {name}@{version} from {source}",
            );
        }
    }
}
