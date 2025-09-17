// Re-export from the new service modules
pub use crate::service::cache::{flush_cache_to_disk, load_cache, store_cache};
pub use crate::service::http_client::get_package_info;
pub use crate::service::registry::{ResolvedPackage, resolve, resolve_dependency};
