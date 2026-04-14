//! Version resolution algorithms.

pub mod builder;
pub mod edges;
#[cfg(feature = "native-git")]
pub mod git;
#[cfg(feature = "native-git")]
pub mod http;
pub mod preload;
pub mod registry;
pub mod runtime;
pub mod semver;
pub mod version;
pub mod workspace;
