//! Runtime capability declarations for dependency protocols.
//!
//! Each protocol declares what capabilities it requires. The resolver
//! checks availability before attempting resolution, enabling graceful
//! handling of unsupported protocols (e.g., `git:` in WASM).

use super::Protocol;

/// A runtime capability that a protocol may require.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// HTTP/HTTPS network access (registry, tarballs).
    Network,
    /// Local filesystem access (`file:`, `link:`).
    FileSystem,
    /// Git operations (`git:`, `github:`).
    Git,
}

impl Capability {
    /// Whether this capability is available in the current environment.
    pub fn is_available(self) -> bool {
        match self {
            Self::Network => true,
            #[cfg(target_arch = "wasm32")]
            Self::FileSystem | Self::Git => false,
            #[cfg(not(target_arch = "wasm32"))]
            Self::FileSystem | Self::Git => true,
        }
    }
}

impl Protocol {
    /// Capabilities required to resolve this protocol at runtime.
    ///
    /// Transform-phase protocols (`catalog:`, `workspace:`) return an
    /// empty slice — they are rewritten before resolution.
    pub fn required_capabilities(self) -> &'static [Capability] {
        match self {
            Self::Catalog | Self::Workspace => &[],
            Self::File | Self::Link | Self::Portal => &[Capability::FileSystem],
            Self::Git | Self::GitHub => &[Capability::Git, Capability::Network],
            Self::Http => &[Capability::Network],
        }
    }

    /// Whether this protocol can be resolved in the current environment.
    pub fn is_available(self) -> bool {
        self.required_capabilities()
            .iter()
            .all(|cap| cap.is_available())
    }

    /// Whether this protocol is a pre-processing transform
    /// (rewritten before graph building, not resolved at runtime).
    pub fn is_transform(self) -> bool {
        matches!(self, Self::Catalog | Self::Workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_protocols_need_no_capabilities() {
        assert!(Protocol::Catalog.required_capabilities().is_empty());
        assert!(Protocol::Workspace.required_capabilities().is_empty());
    }

    #[test]
    fn test_transform_protocols_always_available() {
        assert!(Protocol::Catalog.is_available());
        assert!(Protocol::Workspace.is_available());
    }

    #[test]
    fn test_is_transform() {
        assert!(Protocol::Catalog.is_transform());
        assert!(Protocol::Workspace.is_transform());
        assert!(!Protocol::Git.is_transform());
        assert!(!Protocol::File.is_transform());
        assert!(!Protocol::Http.is_transform());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_native_all_available() {
        assert!(Protocol::File.is_available());
        assert!(Protocol::Link.is_available());
        assert!(Protocol::Git.is_available());
        assert!(Protocol::GitHub.is_available());
        assert!(Protocol::Http.is_available());
    }
}
