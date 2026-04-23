//! Shared utilities usable across ruborist and its downstream crates.

// OnceMap uses `dashmap` + `tokio::sync::Notify`. Both nominally compile
// for wasm, but the only consumers so far (pm-side downloader / cloner,
// and the ruborist preload dedup) are native-only paths. Gate it to keep
// the wasm surface minimal until a wasm consumer actually needs it.
#[cfg(not(target_arch = "wasm32"))]
pub mod oncemap;

// Previously these lived in an inline `pub mod util { ... }` re-export
// block in `lib.rs`; keep the same public path to avoid breaking
// downstream consumers.
pub use crate::model::util::{PackageNameStr, parse_package_spec, read_package_json};
