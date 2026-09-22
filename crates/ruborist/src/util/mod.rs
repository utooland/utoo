//! Shared utility primitives for ruborist and downstream consumers.

pub mod cpu;
pub mod error;
pub mod oncemap;
#[cfg(not(target_arch = "wasm32"))]
pub mod task;

pub use crate::model::util::{PackageNameStr, parse_package_spec, read_package_json};
pub use cpu::spawn_cpu;
pub use oncemap::OnceMap;
