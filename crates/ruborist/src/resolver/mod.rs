//! Version resolution algorithms.

pub mod builder;
pub mod common;
pub mod edges;
// fast_preload + mb_resolve build Send-bound async futures
// (`Pin<Box<dyn Future + Send>>`) over reqwest. wasm32's reqwest holds
// `Rc<RefCell<wasm_bindgen_futures::Inner>>` which is !Send → won't
// compile on wasm. Native callers route through these for the resolve
// hot path; wasm callers stay on the legacy preload + BFS path in
// `service::api::build_deps`.
#[cfg(not(target_arch = "wasm32"))]
pub mod fast_preload;
#[cfg(feature = "native-git")]
pub mod git;
#[cfg(feature = "http-tarball")]
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod mb_resolve;
pub mod preload;
pub mod registry;
pub mod runtime;
pub mod semver;
#[cfg(feature = "http-tarball")]
pub(crate) mod tar;
pub mod version;
pub mod workspace;
