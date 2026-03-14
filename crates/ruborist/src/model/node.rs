//! Node and edge type definitions for the dependency graph.

/// Edge type representing the relationship between packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    /// Production dependency
    Prod,
    /// Development dependency
    Dev,
    /// Peer dependency
    Peer,
    /// Optional dependency
    Optional,
}

/// Controls whether peer dependencies are included during edge iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerDeps {
    /// Include peer dependencies (modern default).
    Include,
    /// Skip peer dependencies (legacy npm behaviour, `--legacy-peer-deps`).
    Skip,
}

/// Controls whether dev dependencies are included during edge iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevDeps {
    /// Include dev dependencies (root packages).
    Include,
    /// Exclude dev dependencies (transitive packages).
    Exclude,
}

/// Node type representing the kind of package in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    /// Root project package
    Root,
    /// Regular npm package
    Regular,
    /// Workspace package (monorepo member)
    Workspace,
    /// Symlinked package
    Link,
}
