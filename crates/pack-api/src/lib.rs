#![feature(str_split_remainder)]
#![feature(impl_trait_in_assoc_type)]
#![feature(arbitrary_self_types)]
#![feature(arbitrary_self_types_pointers)]
#![feature(iter_intersperse)]
#![allow(unexpected_cfgs)]

pub mod app;
pub mod endpoint;
pub mod entrypoint;
pub mod hmr;
pub mod library;
pub mod operation;
pub mod paths;
pub mod project;
pub mod source_map;
pub mod turbo_tasks;
pub mod utils;
pub mod versioned_content_map;
pub mod webpack_stats;
