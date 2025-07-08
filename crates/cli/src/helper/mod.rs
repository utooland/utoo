pub mod auto_update;
pub mod cli;
pub mod compatibility;
pub mod deps;
pub mod install_runtime;
pub mod lock;
pub mod package;
pub mod ruborist;
pub mod workspace;

pub use compatibility::{is_cpu_compatible, is_os_compatible};
