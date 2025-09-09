// Re-export from the new service modules
pub use crate::service::cache::{store_cache, load_cache, flush_cache_to_disk };
pub use crate::service::registry::{resolve, resolve_dependency, ResolvedPackage};
pub use crate::service::http_client::get_package_info;
