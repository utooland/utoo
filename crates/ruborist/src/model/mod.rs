//! Core data structures for dependency resolution.
//!
//! # Type Hierarchy
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Registry Data (from npm API)                                   │
//! │                                                                │
//! │   FullManifest                 1 package, all versions          │
//! │   ├── name, dist_tags, time                                    │
//! │   ├── versions: Vec<String>          (version keys only)       │
//! │   └── raw: Bytes                     (HTTP response bytes)     │
//! │              │                                                 │
//! │              └──► extract_version(ver) ──on demand──►          │
//! │                                                                │
//! │   CoreVersionManifest          1 specific version (hot path)   │
//! │   ├── name, version, dist      (13 fields for resolve/install) │
//! │   ├── dependencies             (extracted & Arc-shared)        │
//! │   ├── peer_dependencies                                        │
//! │   └── optional_dependencies          ▼                         │
//! │                        Arc<CoreVersionManifest>                 │
//! │                        (shared across cache + graph)           │
//! │                                                                │
//! │   VersionManifest              full 28 fields (cold path)      │
//! │   └── used only by `ut view` via get_full_version()            │
//! └─────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Local Data (from disk)                                         │
//! │                                                                │
//! │   PackageJson                  project's package.json          │
//! │   ├── name, version                                            │
//! │   ├── dependencies, dev_dependencies                           │
//! │   ├── peer_dependencies, optional_dependencies                 │
//! │   └── workspaces, engines, bin, ...                            │
//! └─────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Unified Abstraction                                            │
//! │                                                                │
//! │   NodeManifest                                                 │
//! │   ├── Local(PackageJson)                  ← root / workspace   │
//! │   └── Registry(Arc<CoreVersionManifest>)  ← resolved dep      │
//! │                                                                │
//! │   Provides unified accessors:                                  │
//! │     name(), version(), dependencies(), peer_dependencies(),    │
//! │     optional_dependencies(), dev_dependencies(), engines(),    │
//! │     bin(), has_install_scripts()                               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Dependency Graph
//!
//! ```text
//!   DependencyGraph (petgraph DiGraph)
//!   │
//!   ├── Nodes: PackageNode
//!   │   ├── name, version, path
//!   │   ├── manifest: NodeManifest       ← owns package data
//!   │   └── node_type: Root | Regular | Workspace | Link
//!   │
//!   └── Edges: DependencyEdge / PhysicalEdge
//!       ├── DependencyEdge: name + spec + EdgeType(Prod|Dev|Peer|Optional)
//!       └── PhysicalEdge: parent → child (node_modules nesting)
//! ```
//!
//! # Ownership & Sharing
//!
//! `CoreVersionManifest` is the hot type in the resolution pipeline — it flows
//! from registry/cache into graph nodes. Wrapping it in `Arc` avoids deep
//! cloning at every cache read and graph insertion:
//!
//! ```text
//!   ManifestState ──┐
//!                   ├── Arc<CoreVersionManifest> ── (ref-count clone)
//!   PackageNode ────┘
//!
//!   Cold paths (disk I/O, serde) still use owned CoreVersionManifest,
//!   wrapping in Arc::new() at the boundary.
//! ```
//!
//! # Deserialization: Full vs Lightweight Views
//!
//! `PackageJson` is the **full** model — every field ruborist cares about.
//! It is used by the resolver (root project, workspace members) where all
//! fields may be needed.
//!
//! For PM-side operations on already-installed packages (`node_modules/`),
//! parsing the full struct is wasteful and fragile (third-party packages
//! may contain non-standard fields that break strict parsing). Lightweight
//! **View** structs solve this — each declares only the fields it needs;
//! serde silently skips the rest.
//!
//! ```text
//!   package.json on disk
//!   │
//!   ├─ load_package_json::<PackageJson>()    full parse (resolver)
//!   │
//!   └─ load_package_json::<View>()           partial parse (PM hot paths)
//!       │
//!       ├── ScriptsView          { scripts }
//!       │   └─ rebuild: check lifecycle scripts (preinstall/postinstall)
//!       │
//!       ├── IdentityView         { name, version }
//!       │   └─ cache validation: compare installed vs expected
//!       │
//!       ├── PackageInstallView   { name, bin, scripts }
//!       │   └─ post-install: bin linking + lifecycle scripts
//!       │
//!       ├── DepsView             { deps, devDeps, peerDeps, optDeps }
//!       │   └─ lockfile freshness: detect package.json drift
//!       │
//!       └── EnginesView          { engines }
//!           └─ lockfile freshness: root-only engine requirements check
//!
//!   All Views are Deserialize-only (no Serialize).
//!   load_package_json<T: DeserializeOwned>(path) dispatches by type param.
//! ```

pub mod compatibility;
pub mod git;
pub mod graph;
pub mod manifest;
pub mod node;
pub mod override_rule;
pub mod package_json;
pub mod package_lock;
pub mod tarball_info;
pub(crate) mod util;

pub use util::parse_package_spec;
