//! Regression coverage for optional peers in cold and lockfile-seeded builds.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::builder::{BuildDepsConfig, build_deps_with_config};
use super::edges::{EdgeContext, add_edges_from};
use crate::model::graph::DependencyGraph;
use crate::model::lock_codec::lock_to_graph;
use crate::model::node::{DevDeps, EdgeType, PeerDeps};
use crate::model::package_json::PackageJson;
use crate::model::package_lock::PackageLock;
use crate::service::{ManifestJob, ManifestJobDone, ManifestProvider};
use crate::traits::progress::NoopReceiver;
use crate::traits::registry::RegistryClient;
use crate::traits::registry::mock::{MockError, MockRegistryClient};

#[derive(Clone)]
struct RecordingRegistry {
    inner: MockRegistryClient,
    requests: Arc<Mutex<Vec<String>>>,
    semver: bool,
}

impl RegistryClient for RecordingRegistry {
    type Error = MockError;

    fn supports_semver_resolution(&self) -> bool {
        self.semver
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ManifestProvider for RecordingRegistry {
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error> {
        let name = match &job {
            ManifestJob::Full { name, .. }
            | ManifestJob::Version { name, .. }
            | ManifestJob::ExtractVersion { name, .. } => name,
        };
        self.requests.lock().unwrap().push(name.clone());
        self.inner.execute_manifest_job(job).await
    }
}

fn vite() -> Value {
    json!({
        "name": "vite", "version": "6.4.6",
        "dependencies": { "rollup": "^4.0.0" },
        "peerDependencies": { "sass": "^1.0.0" },
        "peerDependenciesMeta": { "sass": { "optional": true } }
    })
}

fn registry(semver: bool) -> RecordingRegistry {
    let mut inner = MockRegistryClient::new();
    inner.add_package("vite", "6.4.6", serde_json::from_value(vite()).unwrap());
    for (name, version) in [
        ("rollup", "4.34.9"),
        ("rollup", "4.40.0"),
        ("unrelated", "1.0.0"),
        ("sass", "1.0.0"),
        ("sass", "2.0.0"),
    ] {
        inner.add_package(
            name,
            version,
            serde_json::from_value(json!({
                "name": name, "version": version,
                "dist": { "tarball": format!("https://registry.test/{name}-{version}.tgz") }
            }))
            .unwrap(),
        );
    }
    inner.set_latest("rollup", "4.40.0");
    RecordingRegistry {
        inner,
        requests: Arc::default(),
        semver,
    }
}

fn baseline(vite: Value) -> PackageLock {
    serde_json::from_value(json!({
        "name": "root", "version": "1.0.0", "lockfileVersion": 3,
        "packages": {
            "": { "name": "root", "version": "1.0.0", "dependencies": { "vite": "6.4.6" } },
            "node_modules/vite": vite,
            "node_modules/rollup": { "version": "4.34.9", "resolved": "https://registry.test/rollup-4.34.9.tgz" }
        }
    })).unwrap()
}

fn root(override_version: Option<&str>) -> PackageJson {
    let mut value = json!({
        "name": "root", "version": "1.0.0",
        "dependencies": { "vite": "6.4.6", "unrelated": "1.0.0" }
    });
    if let Some(version) = override_version {
        value["overrides"] = json!({ "rollup": version });
    }
    serde_json::from_value(value).unwrap()
}

async fn build(
    pkg: PackageJson,
    baseline: Option<&PackageLock>,
    registry: &RecordingRegistry,
    peers: PeerDeps,
) -> DependencyGraph {
    let path = Path::new("/project");
    let mut graph = DependencyGraph::from_package_json(path.into(), pkg.clone());
    let root = graph.root_index;
    add_edges_from(
        &mut graph,
        root,
        &pkg,
        &EdgeContext::new(peers, DevDeps::Include),
    );
    if let Some(lock) = baseline {
        lock_to_graph(&mut graph, lock, path);
    }
    build_deps_with_config(
        &mut graph,
        registry,
        BuildDepsConfig::default().with_peer_deps(peers),
        &NoopReceiver,
    )
    .await
    .unwrap();
    graph
}

fn to_lock(graph: &DependencyGraph) -> PackageLock {
    let (packages, _) = graph.serialize_to_packages_pruned(Path::new("/project"));
    PackageLock::new("root", "1.0.0", packages)
}

#[tokio::test]
async fn absent_optional_peer_is_not_fetched_with_or_without_overrides() {
    for semver in [false, true] {
        for peers in [PeerDeps::Skip, PeerDeps::Include] {
            for locked in [false, true] {
                for override_version in [None, Some("4.34.9"), Some("4.40.0")] {
                    let registry = registry(semver);
                    let baseline = baseline(vite());
                    let graph = build(
                        root(override_version),
                        locked.then_some(&baseline),
                        &registry,
                        peers,
                    )
                    .await;
                    let out = to_lock(&graph);
                    assert!(!out.packages.contains_key("node_modules/sass"));
                    assert!(out.packages.contains_key("node_modules/unrelated"));
                    assert_eq!(
                        serde_json::to_value(&out.packages["node_modules/vite"]).unwrap()["peerDependenciesMeta"],
                        vite()["peerDependenciesMeta"]
                    );
                    let expected_rollup =
                        override_version.unwrap_or(if locked { "4.34.9" } else { "4.40.0" });
                    assert_eq!(
                        out.packages["node_modules/rollup"].version.as_deref(),
                        Some(expected_rollup),
                        "semver={semver}, peers={peers:?}, locked={locked}, override={override_version:?}"
                    );
                    let requests = registry.requests.lock().unwrap();
                    assert!(
                        !requests.iter().any(|name| name == "sass"),
                        "optional peer prefetched: {requests:?}"
                    );
                    if locked {
                        assert!(!requests.iter().any(|name| name == "vite"));
                        assert_eq!(
                            requests.iter().any(|name| name == "rollup"),
                            override_version.is_some()
                        );
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn required_peers_follow_peer_policy_during_override_revalidation() {
    for metadata in [json!({}), json!({ "sass": { "optional": false } })] {
        for locked in [false, true] {
            for peers in [PeerDeps::Skip, PeerDeps::Include] {
                let mut registry = registry(false);
                let mut vite = vite();
                vite["peerDependenciesMeta"] = metadata.clone();
                registry.inner.add_package(
                    "vite",
                    "6.4.6",
                    serde_json::from_value(vite.clone()).unwrap(),
                );
                let baseline = baseline(vite);
                let graph = build(
                    root(Some("4.40.0")),
                    locked.then_some(&baseline),
                    &registry,
                    peers,
                )
                .await;
                let out = to_lock(&graph);
                assert_eq!(
                    out.packages.contains_key("node_modules/sass"),
                    peers == PeerDeps::Include
                );
                assert_eq!(
                    out.packages["node_modules/rollup"].version.as_deref(),
                    Some("4.40.0")
                );
            }
        }
    }
}

#[tokio::test]
async fn optional_peer_binds_explicit_and_late_suppliers() {
    for locked in [false, true] {
        for late_supplier in [false, true] {
            let mut registry = registry(false);
            let mut pkg = root(Some("4.34.9"));
            if late_supplier {
                for (name, dep) in [("unrelated", "supplier"), ("supplier", "sass")] {
                    registry.inner.add_package(
                        name,
                        "1.0.0",
                        serde_json::from_value(json!({
                            "name": name, "version": "1.0.0", "dependencies": { (dep): "1.0.0" }
                        }))
                        .unwrap(),
                    );
                }
            } else {
                pkg.dependencies
                    .as_mut()
                    .unwrap()
                    .insert("sass".into(), "1.0.0".into());
            }
            let baseline = baseline(vite());
            let graph = build(
                pkg,
                locked.then_some(&baseline),
                &registry,
                PeerDeps::Include,
            )
            .await;
            let vite = graph.find_physical_child(graph.root_index, "vite").unwrap();
            let sass = graph.find_physical_child(graph.root_index, "sass").unwrap();
            let (_, edge) = graph
                .get_dependency_edges(vite)
                .into_iter()
                .find(|(_, e)| e.name == "sass")
                .unwrap();
            assert_eq!(edge.edge_type, EdgeType::Peer);
            assert!(edge.optional_peer && edge.valid);
            assert_eq!(edge.to, Some(sass));
        }
    }
}

#[tokio::test]
async fn supplied_optional_peer_uses_normal_override_resolution() {
    let registry = registry(false);
    let mut pkg = root(None);
    pkg.dependencies
        .as_mut()
        .unwrap()
        .insert("sass".into(), "1.0.0".into());
    pkg.overrides = Some(json!({ "sass": "2.0.0" }));
    let graph = build(pkg, None, &registry, PeerDeps::Include).await;
    let lock = to_lock(&graph);
    assert_eq!(
        lock.packages["node_modules/sass"].version.as_deref(),
        Some("2.0.0")
    );
    let vite = graph.find_physical_child(graph.root_index, "vite").unwrap();
    assert!(
        graph
            .get_dependency_edges(vite)
            .iter()
            .any(|(_, edge)| edge.optional_peer && edge.valid)
    );
}

#[tokio::test]
async fn ordinary_optional_dependencies_still_install() {
    let mut registry = registry(false);
    let mut vite = vite();
    vite["peerDependencies"] = json!({});
    vite["optionalDependencies"] = json!({ "sass": "1.0.0" });
    registry
        .inner
        .add_package("vite", "6.4.6", serde_json::from_value(vite).unwrap());
    let graph = build(root(None), None, &registry, PeerDeps::Include).await;
    assert!(to_lock(&graph).packages.contains_key("node_modules/sass"));
}

#[tokio::test]
async fn local_and_locked_metadata_round_trips_true_false_and_empty_entries() {
    use super::builder::add_workspace_member;

    let meta = json!({ "sass": { "optional": true }, "less": { "optional": false }, "stylus": {} });
    let mut vite = vite();
    vite["peerDependenciesMeta"] = meta.clone();
    let mut pkg = root(None);
    pkg.peer_dependencies_meta = Some(serde_json::from_value(meta.clone()).unwrap());
    let registry = registry(false);
    let mut graph = build(
        pkg.clone(),
        Some(&baseline(vite)),
        &registry,
        PeerDeps::Skip,
    )
    .await;
    let root = graph.root_index;
    add_workspace_member(
        &mut graph,
        root,
        "workspace",
        "/project/packages/workspace".into(),
        &pkg,
        &EdgeContext::new(PeerDeps::Skip, DevDeps::Include),
    );
    let value = serde_json::to_value(to_lock(&graph)).unwrap();
    for path in ["", "packages/workspace", "node_modules/vite"] {
        assert_eq!(value["packages"][path]["peerDependenciesMeta"], meta);
    }
}
