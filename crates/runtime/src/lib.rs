pub mod loader;
pub mod napi;
pub mod ops;
pub mod runtime;
pub mod transpile;

#[cfg(feature = "snapshot")]
pub static UTOO_SNAPSHOT: &[u8] = include_bytes!("snapshot_data.bin");
