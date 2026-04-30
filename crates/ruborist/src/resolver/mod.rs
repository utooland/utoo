//! Version resolution algorithms.

pub mod builder;
pub mod common;
pub mod edges;
#[cfg(feature = "native-git")]
pub mod git;
#[cfg(feature = "http-tarball")]
pub mod http;
// Worker-pool preload uses `tokio::spawn` for background workers, which
// requires `Send` futures. wasm-bindgen-futures aren't `Send`, so we gate
// the whole module to native targets and skip the preload phase on wasm.
#[cfg(not(target_arch = "wasm32"))]
pub mod preload;
pub mod registry;
pub mod runtime;
pub mod semver;
#[cfg(feature = "http-tarball")]
pub(crate) mod tar;
pub mod version;
pub mod workspace;
