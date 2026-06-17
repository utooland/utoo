//! Version resolution — turn a root manifest into a fully resolved graph.
//!
//! ```text
//!   service::api::build_deps
//!        │
//!        ▼
//!   builder ── entry points + spec router ──┬─ Registry → demand:: BFS loop
//!        │                                   │    (state·queue·select·
//!        │                                   │     schedule·driver)
//!        │                                   ├─ Git/GitHub → git
//!        │                                   ├─ Http tarball → http
//!        │                                   └─ file: → file
//!        ▼
//!   placement ── attach the resolved package to the graph
//!        │        (create / reuse node, link edges)
//!        ▼
//!   node_types ── propagate prod/dev/optional/peer over the finished tree
//!
//!   shared leaves: version (pick a version) · semver · registry
//!     (resolve_package) · edges (dep-edge construction) · runtime
//!     (engines→node deps) · workspace (member discovery) · common (dedup
//!     cache + atomic cache-slot commit) · tar (gzip/tar primitives)
//! ```

pub mod builder;
pub mod common;
pub(crate) mod demand;
pub mod edges;
#[cfg(feature = "http-tarball")]
pub mod file;
#[cfg(feature = "native-git")]
pub mod git;
#[cfg(feature = "http-tarball")]
pub mod http;
pub mod node_types;
pub mod placement;
pub mod registry;
pub mod runtime;
pub mod semver;
#[cfg(feature = "http-tarball")]
pub(crate) mod tar;
pub mod version;
pub mod workspace;
