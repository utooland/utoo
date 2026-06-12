//! Version resolution algorithms.

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
