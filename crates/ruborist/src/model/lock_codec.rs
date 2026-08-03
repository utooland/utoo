//! Reconstruct a [`DependencyGraph`] from a `package-lock.json` — the inverse of
//! [`super::package_lock::serialize_to_packages`].
//!
//! [`lock_to_graph`] seeds the *already-resolved* tree the lockfile encodes so
//! the demand resolver only works the **delta** (added/changed/removed direct
//! deps) instead of recomputing — and, under concurrent placement, reshuffling —
//! the whole tree. [`lock_is_consistent`] gates that reuse: an internally
//! incomplete lock cold-resolves instead of seeding dangling edges.
//!
//! Composition with graph init: the caller (`service::api::build_deps`) has
//! already created the root, added its **live** (unresolved) dependency edges,
//! attached workspace members, and settled `workspace:` edges. `lock_to_graph`
//! then inserts every regular (non-root, non-link) lock entry as a pinned node
//! with a synthetic manifest, and seeds its recorded dep edges as resolved.
//!
//! The BFS then only enqueues the live unresolved edges; its reuse probe
//! ([`try_reuse_dependency`]) matches them against seeded nodes with no I/O. A
//! re-resolved (bumped) dep shadows its stale seeded node, which then falls out
//! of the reachable set and is dropped by the prune at serialize time.
//!
//! [`try_reuse_dependency`]: crate::resolver::placement::try_reuse_dependency

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use petgraph::graph::NodeIndex;

use crate::model::graph::{DependencyGraph, PackageNode};
use crate::model::manifest::{CoreVersionManifest, Dist};
use crate::model::node::EdgeType;
use crate::model::package_lock::{License, LockPackage, PackageLock};

/// Seed `graph` with the resolved tree encoded by `lock` (the inverse of
/// [`super::package_lock::serialize_to_packages`]).
///
/// `root_path` is the absolute workspace root the lockfile lives at; seeded node
/// paths are built relative to it. The root and any workspace members are
/// expected to already exist in `graph` (added by the caller) — they are looked
/// up by their lockfile path and used as physical parents for the entries that
/// nest under them, but never recreated.
pub fn lock_to_graph(graph: &mut DependencyGraph, lock: &PackageLock, root_path: &Path) {
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

    // Normalize every lock key to its POSIX form up front and key the whole
    // seeding pass off that. `path_index` already holds the importers under
    // POSIX keys (`rel_lock_path` normalizes), and `parent_lock_path` /
    // `path_index` lookups below all consume this key — so a lock written by an
    // older utoo or a foreign tool with native separators (`packages\m`,
    // `packages\m/node_modules/bar`) resolves its parents and skips its
    // importers exactly like a POSIX one. Skip the entries the live graph
    // already owns:
    //   - the root (`""`),
    //   - every link entry — workspace `node_modules/<name>` symlinks are
    //     created by `add_workspace_member`, `file:<dir>` links are re-resolved
    //     from the live importer edge,
    //   - every workspace-member *source* entry (`packages/<m>`), already a
    //     `Workspace` node in `path_index`. Re-seeding it would add a duplicate
    //     plain node at the member's `node_modules/<name>` slot and clobber the
    //     symlink lock entry in serialization, dropping the on-disk link.
    let mut entries: Vec<(String, &LockPackage)> = lock
        .packages
        .iter()
        .filter(|(path, pkg)| !path.is_empty() && !pkg.is_link())
        .map(|(path, pkg)| (to_lock_key(path).into_owned(), pkg))
        .filter(|(key, _)| !path_index.contains_key(key))
        .collect();
    // `sort_by_cached_key` evaluates `lock_path_depth` (string matching) once
    // per entry rather than O(N log N) times. Parent-before-child so every
    // physical parent already exists when its children are inserted.
    entries.sort_by_cached_key(|(key, _)| lock_path_depth(key));

    // Carry the `LockPackage` borrow into the second pass so edges resolve
    // against the fully-populated index (a dep can hoist to any ancestor's
    // node_modules) without re-keying into `lock.packages`.
    let mut seeded: Vec<(String, NodeIndex, &LockPackage)> = Vec::with_capacity(entries.len());

    for (key, pkg) in entries {
        let Some(parent_index) = path_index.get(parent_lock_path(&key)).copied() else {
            // A parent that isn't in the graph means a malformed/partial lock;
            // skip this entry rather than panic — the BFS will resolve it fresh
            // if the live tree still needs it. (`lock_is_consistent` already
            // rejects such locks, so this is a defensive belt-and-braces skip.)
            tracing::debug!("seed: no parent for lock entry {key:?}, skipping");
            continue;
        };

        // A node's name is its node_modules *directory* segment — the slot key
        // the cold resolver hoists and serializes by. For an `npm:` alias this
        // differs from the package's real name: the entry at
        // `node_modules/string-width-cjs` records `name: "string-width"`. Using
        // the real name here would collide an aliased copy with the genuine
        // package in the name-keyed child index — one loses its slot and is
        // pruned, leaving a dangling dependency in the re-emitted lock. The real
        // name (for the manifest) comes from `get_name`; the slot name from the
        // path, falling back to the real name for paths with no `node_modules/`
        // segment (e.g. a workspace member).
        let real_name = pkg.get_name(&key);
        let slot_name = LockPackage::path_to_pkg_name(&key)
            .map(str::to_string)
            .unwrap_or_else(|| real_name.clone());
        let manifest = Arc::new(lock_package_to_manifest(&real_name, pkg));
        let node = PackageNode::from_version_manifest(slot_name, root_path.join(&key), manifest);
        let index = graph.add_node(node);
        graph.add_physical_edge(parent_index, index);
        path_index.insert(key.clone(), index);
        seeded.push((key, index, pkg));
    }

    // Second pass: seed each node's recorded dependency edges as resolved,
    // pointing at the node that occupies the hoisted slot the lock placed them
    // in. This keeps the seeded subtree connected without the BFS touching it.
    for (key, index, pkg) in seeded {
        seed_resolved_edges(graph, &key, index, pkg, &path_index);
    }
}

/// Normalize a lockfile path to its POSIX form (npm lock keys use `/`). Borrows
/// when the path is already POSIX, so the common case allocates nothing. The one
/// place separator normalization happens for reuse, shared by [`lock_to_graph`],
/// [`lock_is_consistent`], and [`rel_lock_path`].
fn to_lock_key(path: &str) -> Cow<'_, str> {
    if path.contains('\\') {
        Cow::Owned(path.replace('\\', "/"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Add `pkg`'s recorded dependency edges from the node at `index`, marking each
/// resolved against the slot the lockfile hoisted it to. A target may still be
/// absent for a non-prod edge (an optional/peer dep with no entry) or one that
/// resolves through a skipped link; such edges are left unresolved — they
/// round-trip into the re-emitted lock entry but never enter the BFS, since a
/// seeded node is not a freshly-created one. Prod-dep targets are guaranteed
/// present: [`lock_is_consistent`] gates seeding on every prod dep resolving.
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

/// Walk the npm hoisting chain from `from_path` upward, returning the lockfile
/// path that satisfies `dep_name` at the nearest ancestor `node_modules/` for
/// which `exists` is true — mirroring how the lockfile's physical layout
/// resolves a dependency. Ancestors are reached by stripping the trailing
/// `/node_modules/<seg>`; a path with no such segment (a workspace member) falls
/// back to the project root. Shared by [`resolve_dep_target`] (against the
/// graph's path index) and [`lock_is_consistent`] (against the lock's keys).
fn resolve_dep_path(
    from_path: &str,
    dep_name: &str,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut search: &str = from_path;
    loop {
        let candidate = if search.is_empty() {
            format!("node_modules/{dep_name}")
        } else {
            format!("{search}/node_modules/{dep_name}")
        };
        if exists(&candidate) {
            return Some(candidate);
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

/// The seeded node that satisfies `dep_name` for the package at `from_path`.
fn resolve_dep_target(
    path_index: &HashMap<String, NodeIndex>,
    from_path: &str,
    dep_name: &str,
) -> Option<NodeIndex> {
    resolve_dep_path(from_path, dep_name, |p| path_index.contains_key(p))
        .and_then(|p| path_index.get(&p).copied())
}

/// Read-only index for resolving dependency edges against a lockfile's
/// physical `node_modules` layout.
///
/// Building the normalized path map once keeps query commands at O(packages +
/// direct dependencies * tree depth), rather than scanning every lock entry
/// for every direct dependency.
pub struct LockDependencyIndex<'a> {
    packages: HashMap<Cow<'a, str>, (&'a str, &'a LockPackage)>,
}

impl<'a> LockDependencyIndex<'a> {
    pub fn new(lock: &'a PackageLock) -> Self {
        let packages = lock
            .packages
            .iter()
            .map(|(path, package)| (to_lock_key(path), (path.as_str(), package)))
            .collect();
        Self { packages }
    }

    /// Resolve one dependency edge from an importer/package lock path to the
    /// nearest package entry selected by npm's ancestor lookup.
    pub fn resolve(&self, from_path: &str, dep_name: &str) -> Option<(&'a str, &'a LockPackage)> {
        let from_path = to_lock_key(from_path);
        let resolved = resolve_dep_path(&from_path, dep_name, |candidate| {
            self.packages.contains_key(candidate)
        })?;
        self.packages.get(resolved.as_str()).copied()
    }
}

/// Whether `lock` is internally complete enough to seed: every **prod**
/// dependency recorded in a regular (non-root, non-link) entry resolves — via
/// npm hoisting — to *some* existing lock entry.
///
/// A `false` means the baseline is incomplete (hand-edited, a partial/aborted
/// install, or a foreign lock shaped differently than ours): seeding it would
/// leave those transitive edges dangling — listed in the re-emitted lock but
/// never installed (a runtime `MODULE_NOT_FOUND`) — so the caller must
/// cold-resolve instead of reusing it.
///
/// Only prod deps are checked: optional/peer deps may legitimately have no
/// entry (an unmet peer, or an optional dep skipped on this platform). The
/// existence set includes link entries, so a cross-workspace dep satisfied
/// through a `node_modules/<member>` symlink is not flagged.
pub(crate) fn lock_is_consistent(lock: &PackageLock) -> bool {
    // POSIX-normalized key set: an older utoo / foreign lock may record native
    // separators, and seeding keys everything off the normalized form, so the
    // gate must judge against the same view (else it could pass a lock whose
    // nested keys seeding then fails to wire — a seeded-but-dangling edge).
    let keys: HashSet<Cow<str>> = lock.packages.keys().map(|k| to_lock_key(k)).collect();
    let exists = |p: &str| keys.contains(p);
    lock.packages.iter().all(|(path, pkg)| {
        if path.is_empty() || pkg.is_link() {
            return true;
        }
        let key = to_lock_key(path);
        // Every entry's physical parent must exist, or seeding finds no node to
        // attach under and silently drops this subtree. The root ("") is
        // implicit; a workspace-member source (`packages/<m>`) is a real entry,
        // so it's in `keys`. A missing intermediate → cold-resolve instead.
        let parent = parent_lock_path(&key);
        if !parent.is_empty() && !keys.contains(parent) {
            return false;
        }
        pkg.dependencies
            .iter()
            .flatten()
            .all(|(dep_name, _)| resolve_dep_path(&key, dep_name, exists).is_some())
    })
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
    Some(to_lock_key(&rel.to_string_lossy()).into_owned())
}

/// Build a [`CoreVersionManifest`] from a lockfile entry. The lock records
/// everything the resolver and the round-trip serializer need for a pinned
/// node: version, the tarball/integrity (as `dist`), the dependency maps, and
/// the package metadata (`bin`/`engines`/`os`/`cpu`/`scripts`/`license`) — so
/// the lockfile is itself the warm cache on the reuse path.
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
    use std::path::PathBuf;

    use crate::model::graph::FindResult;
    use crate::model::node::{DevDeps, PeerDeps};
    use crate::model::package_json::PackageJson;
    use crate::resolver::builder::{add_workspace_member, resolve_workspace_member_edges};
    use crate::resolver::edges::{EdgeContext, add_edges_from};

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

        lock_to_graph(&mut graph, lock, &root_path);

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
        lock_to_graph(&mut graph, &lock, &root_path);

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

    /// A workspace's `node_modules/<name>` symlink must survive pruning on the
    /// reuse path. `add_workspace_member` attaches a `Link` node whose only
    /// incoming edge is physical (the importer edge resolves to the `Workspace`
    /// node, not the link), so a reachability sweep over resolved edges alone
    /// would drop it — deleting the symlink that makes the member importable by
    /// name. Regression test for that.
    #[test]
    fn test_prune_keeps_workspace_link() {
        let root_path = PathBuf::from("/proj");
        let mut root_pkg = PackageJson::new("wsroot", "1.0.0");
        root_pkg.dependencies = Some(HashMap::from([
            ("foo".to_string(), "workspace:*".to_string()),
            ("a".to_string(), "^1.0.0".to_string()),
        ]));
        let mut graph = DependencyGraph::from_package_json(root_path.clone(), root_pkg.clone());
        let root = graph.root_index;
        let ctx = EdgeContext::new(PeerDeps::Skip, DevDeps::Include);
        add_edges_from(&mut graph, root, &root_pkg, &ctx);

        // Attach workspace member `foo` — this creates both the Workspace node
        // (`packages/foo`) and its `node_modules/foo` Link node.
        let foo_pkg = PackageJson::new("foo", "1.0.0");
        add_workspace_member(
            &mut graph,
            root,
            "foo",
            root_path.join("packages/foo"),
            &foo_pkg,
            &ctx,
        );
        resolve_workspace_member_edges(&mut graph);

        // Seed a prior lock that pins the regular dep `a` (workspace links are
        // never seeded — they're rebuilt by `add_workspace_member` above).
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert("node_modules/a".to_string(), lock_entry("1.0.0", &[]));
        let lock = PackageLock::new("wsroot", "1.0.0", packages);
        lock_to_graph(&mut graph, &lock, &root_path);

        // Settle the root's live edges (reuse probe stand-in).
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

        let (pruned, _) = graph.serialize_to_packages_pruned(&root_path);
        let link = pruned
            .get("node_modules/foo")
            .expect("workspace symlink entry must survive prune");
        assert_eq!(link.link, Some(true), "kept entry is the symlink");
        assert!(pruned.contains_key("packages/foo"), "workspace member kept");
        assert!(
            pruned.contains_key("node_modules/a"),
            "pinned regular dep kept"
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

    #[test]
    fn lock_dependency_index_resolves_nearest_hoisted_entry() {
        let lock = PackageLock::new(
            "root",
            "1.0.0",
            HashMap::from([
                (String::new(), root_entry(&[])),
                ("node_modules/dep".to_string(), lock_entry("1.0.0", &[])),
                (
                    "node_modules/a/node_modules/dep".to_string(),
                    lock_entry("2.0.0", &[]),
                ),
            ]),
        );
        let index = LockDependencyIndex::new(&lock);

        let (nested_path, nested) = index.resolve("node_modules/a", "dep").unwrap();
        assert_eq!(nested_path, "node_modules/a/node_modules/dep");
        assert_eq!(nested.version.as_deref(), Some("2.0.0"));

        let (hoisted_path, hoisted) = index.resolve("packages/workspace", "dep").unwrap();
        assert_eq!(hoisted_path, "node_modules/dep");
        assert_eq!(hoisted.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_lock_is_consistent_complete() {
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        let lock = PackageLock::new("root", "1.0.0", packages);
        assert!(lock_is_consistent(&lock));
    }

    #[test]
    fn test_lock_is_inconsistent_missing_transitive() {
        // `a` records a prod dep on `b`, but `b` has no lock entry — seeding
        // would dangle, so this must be rejected (→ cold resolve).
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        let lock = PackageLock::new("root", "1.0.0", packages);
        assert!(!lock_is_consistent(&lock));
    }

    #[test]
    fn test_lock_is_consistent_ignores_missing_optional() {
        // An optional dep with no entry (skipped on this platform) is fine.
        let mut a = lock_entry("1.0.0", &[]);
        a.optional_dependencies = Some(HashMap::from([("fsevents".to_string(), "^2".to_string())]));
        let mut packages = HashMap::new();
        packages.insert(String::new(), root_entry(&[("a", "^1.0.0")]));
        packages.insert("node_modules/a".to_string(), a);
        let lock = PackageLock::new("root", "1.0.0", packages);
        assert!(lock_is_consistent(&lock));
    }

    #[test]
    fn test_lock_is_consistent_nested_hoist() {
        // A nested copy resolves its dep against its own nested sibling.
        let mut packages = HashMap::new();
        packages.insert(
            String::new(),
            root_entry(&[("a", "^1.0.0"), ("c", "^1.0.0")]),
        );
        packages.insert(
            "node_modules/a".to_string(),
            lock_entry("1.0.0", &[("b", "^1.0.0")]),
        );
        packages.insert("node_modules/b".to_string(), lock_entry("1.0.0", &[]));
        packages.insert(
            "node_modules/c".to_string(),
            lock_entry("1.0.0", &[("a", "^2.0.0")]),
        );
        packages.insert(
            "node_modules/c/node_modules/a".to_string(),
            lock_entry("2.0.0", &[("b", "^2.0.0")]),
        );
        packages.insert(
            "node_modules/c/node_modules/b".to_string(),
            lock_entry("2.0.0", &[]),
        );
        let lock = PackageLock::new("root", "1.0.0", packages);
        assert!(lock_is_consistent(&lock));
    }
}
