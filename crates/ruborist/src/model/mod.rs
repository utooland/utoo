//! Core data structures for dependency resolution.

pub mod compatibility;
pub mod graph;
pub mod manifest;
pub mod node;
pub mod override_rule;
pub mod package_json;
pub mod package_lock;
pub mod tarball_info;
pub(crate) mod util;

pub use util::parse_package_spec;
