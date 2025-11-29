#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeType {
    Prod,     // Production dependency
    Dev,      // Development dependency
    Peer,     // Peer dependency
    Optional, // Optional dependency
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Root,
    Regular,
    Workspace,
    Link,
}
