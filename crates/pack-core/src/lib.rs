#![feature(arbitrary_self_types_pointers)]
#![feature(box_patterns)]
#![feature(str_as_str)]

pub mod client;
pub mod config;
pub mod embed_js;
pub mod emit;
pub mod import_map;
pub mod library;
pub mod mode;
pub mod server;
pub mod server_component;
pub mod shared;
pub mod tracing_presets;
pub mod transform_options;
pub mod util;

pub use emit::{all_assets_from_entries, emit_assets};

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_bytes::register();
    turbo_tasks_fs::register();
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    turbo_tasks_fetch::register();
    turbopack_browser::register();
    turbopack_node::register();
    turbopack::register();
    turbopack_image::register();
    turbopack_ecmascript::register();
    turbopack_ecmascript_plugins::register();
    turbo_esregex::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
