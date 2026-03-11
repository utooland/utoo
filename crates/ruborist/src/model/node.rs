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
