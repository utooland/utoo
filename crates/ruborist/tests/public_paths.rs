//! Keep the pre-refactor public paths type-compatible with the facade.
#[test]
fn public_manifest_lock_graph_and_options_paths_remain_compatible() {
    use utoo_ruborist::{graph, lock, manifest, model, resolver, service};
    let package = manifest::PackageJson::default();
    let _: model::package_json::PackageJson = package.clone();
    let graph = graph::DependencyGraph::from_package_json(".".into(), package);
    let _: model::graph::DependencyGraph = graph;
    let lock: Option<lock::PackageLock> = None;
    let _: Option<model::package_lock::PackageLock> = lock;
    let options: Option<
        service::BuildDepsOptions<service::NoopGlob, utoo_ruborist::progress::NoopReceiver>,
    > = None;
    assert!(options.is_none());
    let _: resolver::builder::PeerDeps = utoo_ruborist::builder::PeerDeps::Include;
}
