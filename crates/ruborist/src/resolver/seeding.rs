//! Seed a [`DependencyGraph`] from an existing `package-lock.json`.
//!
//! This is the inverse of [`crate::model::package_lock::serialize_to_packages`]:
//! it reconstructs the *already-resolved* tree encoded by the lockfile so the
//! demand resolver only has to do work for the **delta** — newly added,
//! changed, or removed direct dependencies — instead of recomputing (and, under
//! concurrent placement, reshuffling) the whole tree.
//!
//! How it composes with graph init:
//! - The caller (`service::api::build_deps`) has already created the root node,
//!   added the root's **live** dependency edges from the current `package.json`
//!   (unresolved), attached workspace members, and settled `workspace:` edges.
//! - [`seed_graph_from_lock`] then inserts every *regular* (non-root,
//!   non-link) lockfile entry as a pinned node at its locked physical path, with
//!   a synthetic manifest derived from the lock entry, and seeds that node's
//!   recorded dependency edges as **already resolved**.
//!
//! The BFS that follows only enqueues *unresolved* edges (the live importer
//! edges), and its pre-fetch reuse probe ([`try_reuse_dependency`]) matches them
//! against the seeded nodes with no network I/O. Pinned transitive edges are
//! never re-examined. Anything the loop re-resolves (a bumped direct dep) is
//! placed fresh and shadows the stale seeded node in the last-wins child index;
//! the stale node then falls out of the reachable set and is dropped by the
//! pruning pass at serialize time.
//!
//! [`try_reuse_dependency`]: crate::resolver::placement::try_reuse_dependency

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::manifest::{CoreVersionManifest, Dist};
use crate::model::node::EdgeType;
use crate::model::package_lock::{License, LockPackage, PackageLock};

/// Seed `graph` with the resolved tree encoded by `lock`.
///
/// `root_path` is the absolute workspace root the lockfile lives at; seeded node
/// paths are built relative to it. The root and any workspace members are
/// expected to already exist in `graph` (added by the caller) — they are looked
/// up by their lockfile path and used as physical parents for the entries that
/// nest under them, but never recreated.
pub fn seed_graph_from_lock(graph: &mut DependencyGraph, lock: &PackageLock, root_path: &Path) {
    // Map a lockfile path (e.g. `""`, `node_modules/a`, `packages/ws`) to the
    // node that occupies it. Pre-populated with the importers the caller built
    // (root + workspace members) so nested entries can find their parent.
    let mut path_index: HashMap<String, NodeIndex> = HashMap::new();
    path_index.insert(String::new(), graph.root_index);
    for ws_index in graph.get_workspace_nodes() {
        if let Some(node) = graph.get_node(ws_index)
            && let Some(key) = rel_lock_path(&node.path, root_path)
        {
            path_index.insert(key, ws_index);
        }
    }

    // Insert entries parent-before-child so every physical parent already
    // exists. Lockfile depth = number of `node_modules/` segments on the path.
    let mut entries: Vec<(&String, &LockPackage)> = lock
        .packages
        .iter()
        // Skip the root entry (already in the graph) and every link entry:
        // workspace links are created by `add_workspace_member`, and `file:`
        // links are re-resolved from the live importer edge when still declared.
        .filter(|(path, pkg)| !path.is_empty() && !pkg.is_link())
        .collect();
    entries.sort_by_key(|(path, _)| lock_path_depth(path));

    // Record (path, node) for the second pass so edges resolve against the
    // fully-populated index (a dep can hoist to any ancestor's node_modules).
    let mut seeded: Vec<(String, NodeIndex)> = Vec::with_capacity(entries.len());

    for (path, pkg) in entries {
        let Some(parent_index) = path_index.get(parent_lock_path(path)).copied() else {
            // A parent that isn't in the graph means a malformed/partial lock;
            // skip this entry rather than panic — the BFS will resolve it fresh
            // if the live tree still needs it.
            tracing::debug!("seed: no parent for lock entry {path:?}, skipping");
            continue;
        };

        let name = pkg.get_name(path);
        let manifest = Arc::new(lock_package_to_manifest(&name, pkg));
        let node =
            PackageNode::from_version_manifest(name, root_path.join(path), Arc::clone(&manifest));
        let index = graph.add_node(node);
        graph.add_physical_edge(parent_index, index);
        path_index.insert(path.clone(), index);
        seeded.push((path.clone(), index));
    }

    // Second pass: seed each node's recorded dependency edges as resolved,
    // pointing at the node that occupies the hoisted slot the lock placed them
    // in. This keeps the seeded subtree connected without the BFS touching it.
    for (path, index) in seeded {
        let Some(pkg) = lock.packages.get(&path) else {
            continue;
        };
        seed_resolved_edges(graph, &path, index, pkg, &path_index);
    }
}

/// Add `pkg`'s recorded dependency edges from the node at `index`, marking each
/// resolved against the slot the lockfile hoisted it to. Edges whose target is
/// absent (a skipped link, or an inconsistent lock) are left unresolved — they
/// round-trip into the re-emitted lock entry but never enter the BFS, since a
/// seeded node is not a freshly-created one.
fn seed_resolved_edges(
    graph: &mut DependencyGraph,
    path: &str,
    index: NodeIndex,
    pkg: &LockPackage,
    path_index: &HashMap<String, NodeIndex>,
) {
    let groups = [
        (pkg.dependencies.as_ref(), EdgeType::Prod),
        (pkg.optional_dependencies.as_ref(), EdgeType::Optional),
        (pkg.peer_dependencies.as_ref(), EdgeType::Peer),
        (pkg.dev_dependencies.as_ref(), EdgeType::Dev),
    ];
    for (deps, edge_type) in groups {
        for (dep_name, dep_spec) in deps.into_iter().flatten() {
            let edge_id = graph.add_dependency_edge(index, dep_name, dep_spec, edge_type);
            if let Some(target) = resolve_dep_target(path_index, path, dep_name) {
                graph.mark_dependency_resolved(edge_id, target);
            } else {
                tracing::debug!("seed: unresolved {dep_name} from {path:?}");
            }
        }
    }
}

/// Walk the npm hoisting chain from `from_path` upward, returning the node that
/// satisfies `dep_name` at the nearest ancestor `node_modules/` — mirroring how
/// the lockfile's physical layout resolves a dependency. Ancestors are reached
/// by stripping the trailing `/node_modules/<seg>`; a path with no such segment
/// (a workspace member) falls back to the project root.
fn resolve_dep_target(
    path_index: &HashMap<String, NodeIndex>,
    from_path: &str,
    dep_name: &str,
) -> Option<NodeIndex> {
    let mut search: &str = from_path;
    loop {
        let candidate = if search.is_empty() {
            format!("node_modules/{dep_name}")
        } else {
            format!("{search}/node_modules/{dep_name}")
        };
        if let Some(&index) = path_index.get(&candidate) {
            return Some(index);
        }
        if search.is_empty() {
            return None;
        }
        search = match search.rfind("/node_modules/") {
            Some(idx) => &search[..idx],
            None => "",
        };
    }
}

/// The lockfile path of the physical parent: everything before the final
/// `node_modules/` segment. `node_modules/a` → `""` (root); `a/node_modules/b`
/// → `a`; `node_modules/x/node_modules/y` → `node_modules/x`.
fn parent_lock_path(path: &str) -> &str {
    match path.rfind("node_modules/") {
        Some(idx) => path[..idx].trim_end_matches('/'),
        None => "",
    }
}

/// Number of `node_modules/` segments — the tree depth used to order seeding so
/// a parent is always inserted before its children.
fn lock_path_depth(path: &str) -> usize {
    path.matches("node_modules/").count()
}

/// Relative POSIX-style lockfile key for a node whose absolute path is `path`.
/// Returns `None` when `path` is not under `root_path`.
fn rel_lock_path(path: &Path, root_path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root_path).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Build a [`CoreVersionManifest`] from a lockfile entry. The lock records
/// everything the resolver and the round-trip serializer need for a pinned
/// node: version, the tarball/integrity (as `dist`), the dependency maps, and
/// the package metadata (`bin`/`engines`/`os`/`cpu`/`scripts`/`license`). This
/// is why the on-disk `.utoo-manifest.json` project cache is redundant on the
/// reuse path — the lockfile is itself the warm cache.
fn lock_package_to_manifest(name: &str, pkg: &LockPackage) -> CoreVersionManifest {
    CoreVersionManifest {
        name: name.to_string(),
        version: pkg.get_version(),
        dependencies: pkg.dependencies.clone(),
        dev_dependencies: pkg.dev_dependencies.clone(),
        peer_dependencies: pkg.peer_dependencies.clone(),
        optional_dependencies: pkg.optional_dependencies.clone(),
        dist: Dist {
            tarball: pkg.resolved.clone(),
            integrity: pkg.integrity.clone(),
            ..Dist::default()
        },
        // `bin`/`os`/`cpu` share the lock's typed representation, so they carry
        // straight over into the manifest fields.
        bin: pkg.bin.clone(),
        engines: pkg.engines.clone(),
        os: pkg.os.clone(),
        cpu: pkg.cpu.clone(),
        scripts: pkg.scripts.clone(),
        has_install_script: pkg.has_install_script,
        license: match &pkg.license {
            Some(License::String(s)) => Some(s.clone()),
            // An array license can't round-trip into the single-string manifest
            // field; npm drops it on the transitive entry too.
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parent_lock_path() {
        assert_eq!(parent_lock_path("node_modules/lodash"), "");
        assert_eq!(
            parent_lock_path("node_modules/a/node_modules/b"),
            "node_modules/a"
        );
        assert_eq!(
            parent_lock_path("packages/ws/node_modules/bar"),
            "packages/ws"
        );
        assert_eq!(parent_lock_path("node_modules/@scope/pkg"), "");
        assert_eq!(parent_lock_path("packages/ws"), "");
    }

    #[test]
    fn test_lock_path_depth() {
        assert_eq!(lock_path_depth(""), 0);
        assert_eq!(lock_path_depth("packages/ws"), 0);
        assert_eq!(lock_path_depth("node_modules/a"), 1);
        assert_eq!(lock_path_depth("node_modules/a/node_modules/b"), 2);
        assert_eq!(lock_path_depth("packages/ws/node_modules/bar"), 1);
    }

    use crate::model::graph::FindResult;
    use crate::model::node::{DevDeps, PeerDeps};
    use crate::model::package_json::PackageJson;
    use crate::resolver::edges::{EdgeContext, add_edges_from};
    use std::path::PathBuf;

    /// A regular (registry) lock entry with the given version, tarball, and prod deps.
    fn lock_entry(version: &str, deps: &[(&str, &str)]) -> LockPackage {
        LockPackage {
            version: Some(version.to_string()),
            resolved: Some(format!("https://registry.test/{version}.tgz")),
            integrity: Some("sha512-test".to_string()),
            dependencies: (!deps.is_empty()).then(|| {
                deps.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            }),
            ..LockPackage::default()
        }
    }

    fn root_entry(deps: &[(&str, &str)]) -> LockPackage {
        LockPackage {
            name: Some("root".to_string()),
            version: Some("1.0.0".to_string()),
            dependencies: Some(
                deps.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            ..LockPackage::default()
        }
    }

    /// Build a graph seeded from `lock`, then settle the root's live importer
    /// edges against the seeded nodes (standing in for the BFS reuse probe).
    fn seed_and_settle(
        root_deps: &[(&str, &str)],
        lock: &PackageLock,
    ) -> (DependencyGraph, PathBuf) {
        let root_path = PathBuf::from("/proj");
        let mut root_pkg = PackageJson::new("root", "1.0.0");
        root_pkg.dependencies = Some(
            root_deps
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );

        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg.clone());
        let root = graph.root_index;
        add_edges_from(
            &mut graph,
            root,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Skip, DevDeps::Include),
        );

        seed_graph_from_lock(&mut graph, lock, &root_path);

        // Settle the root's live edges the way the BFS's reuse probe would.
        let pending: Vec<_> = graph
            .get_dependency_edges(root)
            .into_iter()
            .filter(|(_, dep)| !dep.valid)
            .map(|(id, dep)| (id, dep.name.clone(), dep.spec.clone()))
            .collect();
        for (id, name, spec) in pending {
            if let FindResult::Reuse(target) = graph.find_compatible_node(root, &name, &spec) {
                graph.mark_dependency_resolved(id, target);
            }
        }
        (graph, root_path)
    }

    /// Seeding a lock then re-emitting it is a fixpoint: an unchanged project
    /// serializes back to the identical tree.
    #[test]
    fn test_seed_then_serialize_is_identity() {
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        let lock = PackageLock::new("root", "1.0.0", packages);

        let (graph, root_path) = seed_and_settle(&[("a", "^1.0.0")], &lock);
        let (out, _) = graph.serialize_to_packages_pruned(&root_path);

        let mut keys: Vec<_> = out.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec!["", "node_modules/a", "node_modules/b"]);
        assert_eq!(out["node_modules/a"].version.as_deref(), Some("1.0.0"));
        assert_eq!(
            out["node_modules/a"].dependencies.as_ref().unwrap()["b"],
            "^1.0.0"
        );
        assert_eq!(out["node_modules/b"].version.as_deref(), Some("1.0.0"));
    }

    /// A lock entry no longer referenced by the live tree (a removed dep and its
    /// exclusively-owned transitive) is pruned from the emitted lock.
    #[test]
    fn test_prune_removed_dependency_subtree() {
        // Lock still records `a`→`b`, but the root no longer depends on `a`.
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        let lock = PackageLock::new("root", "1.0.0", packages);

        // Root manifest declares nothing — `a` and `b` become unreachable.
        let (graph, root_path) = seed_and_settle(&[], &lock);

        let reachable = graph.reachable_nodes();
        assert_eq!(reachable.len(), 1, "only the root is reachable");

        let (pruned, _) = graph.serialize_to_packages_pruned(&root_path);
        assert_eq!(pruned.keys().cloned().collect::<Vec<_>>(), vec![""]);

        // Without pruning, the orphans would still be emitted — proving the
        // filter is what removes them, not the seeding.
        let (unpruned, _) = graph.serialize_to_packages(&root_path);
        assert!(unpruned.contains_key("node_modules/a"));
        assert!(unpruned.contains_key("node_modules/b"));
    }

    /// A shared transitive survives when only one of its dependents is removed.
    #[test]
    fn test_prune_keeps_shared_transitive() {
        // Both `a` and `c` depend on `b` (hoisted). Root keeps `a`, drops `c`.
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        packages.insert(
            "node_modules/c".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        let lock = PackageLock::new("root", "1.0.0", packages);

        let (graph, root_path) = seed_and_settle(&[("a", "^1.0.0")], &lock);
        let (pruned, _) = graph.serialize_to_packages_pruned(&root_path);

        let mut keys: Vec<_> = pruned.keys().cloned().collect();
        keys.sort();
        // `c` is gone; `b` stays (still required by `a`).
        assert_eq!(keys, vec!["", "node_modules/a", "node_modules/b"]);
    }

    /// A bumped direct dep: the BFS places the new version at the same hoisted
    /// slot, shadowing the seeded one in the last-wins child index. The stale
    /// node (and its now-orphaned transitive) must be pruned, and the slot must
    /// serialize as the new version.
    #[test]
    fn test_prune_shadowed_bumped_version() {
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        let lock = PackageLock::new("root", "1.0.0", packages);

        // Root now wants a@^2 — `a@1.0.0` no longer satisfies it.
        let root_path = PathBuf::from("/proj");
        let mut root_pkg = PackageJson::new("root", "1.0.0");
        root_pkg.dependencies = Some(HashMap::from([("a".to_string(), "^2.0.0".to_string())]));
        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg.clone());
        let root = graph.root_index;
        add_edges_from(
            &mut graph,
            root,
            &root_pkg,
            &EdgeContext::new(PeerDeps::Skip, DevDeps::Include),
        );
        seed_graph_from_lock(&mut graph, &lock, &root_path);

        // The live edge can't reuse the seeded a@1.0.0 (spec mismatch).
        let (edge_id, _, _) = graph
            .get_dependency_edges(root)
            .into_iter()
            .find(|(_, d)| d.name == "a")
            .map(|(id, d)| (id, d.name.clone(), d.spec.clone()))
            .unwrap();
        assert!(
            matches!(
                graph.find_compatible_node(root, "a", "^2.0.0"),
                FindResult::Conflict(_)
            ),
            "bumped spec must conflict with the seeded version"
        );

        // Stand in for the BFS placing a@2.0.0 at the root slot.
        let manifest = Arc::new(CoreVersionManifest {
            name: "a".to_string(),
            version: "2.0.0".to_string(),
            ..CoreVersionManifest::default()
        });
        let a2 = graph.add_node(PackageNode::from_version_manifest(
            "a".to_string(),
            root_path.join("node_modules/a"),
            manifest,
        ));
        graph.add_physical_edge(root, a2);
        graph.mark_dependency_resolved(edge_id, a2);

        let (pruned, _) = graph.serialize_to_packages_pruned(&root_path);
        let mut keys: Vec<_> = pruned.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["", "node_modules/a"],
            "stale a@1.0.0 + its b pruned"
        );
        assert_eq!(
            pruned["node_modules/a"].version.as_deref(),
            Some("2.0.0"),
            "slot serializes as the bumped version"
        );
    }

    #[test]
    fn test_resolve_dep_target_hoist_chain() {
        let mut index = HashMap::new();
        index.insert(String::new(), NodeIndex::new(0));
        index.insert("node_modules/dep".to_string(), NodeIndex::new(1));
        index.insert(
            "node_modules/a/node_modules/dep".to_string(),
            NodeIndex::new(2),
        );

        // Nested package prefers its own nested copy.
        assert_eq!(
            resolve_dep_target(&index, "node_modules/a", "dep"),
            Some(NodeIndex::new(2))
        );
        // A different package hoists to the root copy.
        assert_eq!(
            resolve_dep_target(&index, "node_modules/b", "dep"),
            Some(NodeIndex::new(1))
        );
        // Workspace member hoists to the root copy.
        assert_eq!(
            resolve_dep_target(&index, "packages/ws", "dep"),
            Some(NodeIndex::new(1))
        );
        // Unknown dependency.
        assert_eq!(
            resolve_dep_target(&index, "node_modules/a", "missing"),
            None
        );
    }
}
