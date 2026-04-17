#![feature(arbitrary_self_types_pointers)]
#![feature(box_patterns)]
#![allow(unexpected_cfgs)]

pub mod client;
pub mod config;
pub mod embed_js;
pub mod emit;
pub mod import_map;
pub mod library;
pub mod mode;
pub mod node_polyfill;
pub mod server;
pub mod server_reference;
pub mod shared;
pub mod tracing_presets;
pub mod transform_options;
pub mod util;

pub use emit::emit_assets;
