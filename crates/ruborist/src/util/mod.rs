//! Shared utility primitives for ruborist and downstream consumers.

pub mod cpu;
pub mod oncemap;

pub use crate::model::util::{PackageNameStr, parse_package_spec, read_package_json};
pub(crate) use cpu::spawn_cpu;
pub use oncemap::OnceMap;
