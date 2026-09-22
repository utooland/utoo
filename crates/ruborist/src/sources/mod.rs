//! Native package sources and shared content primitives. Workers return
//! manifests/content; the resolver owns placement and graph mutation.
pub mod common;
#[cfg(feature = "http-tarball")]
pub mod file;
#[cfg(feature = "native-git")]
pub mod git;
#[cfg(feature = "http-tarball")]
pub mod http;
#[cfg(feature = "http-tarball")]
pub(crate) mod tar;
