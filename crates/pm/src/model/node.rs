/// Edge type in dependency graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Prod,     // Production dependency
    Dev,      // Development dependency
    Peer,     // Peer dependency
    Optional, // Optional dependency
}

/// Node type in dependency graph
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Root,
    Regular,
    Workspace,
    Link,
}
