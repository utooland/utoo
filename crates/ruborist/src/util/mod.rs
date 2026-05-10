//! Shared utility primitives for ruborist and downstream consumers.

pub mod oncemap;
pub mod timing;

pub use crate::model::util::{PackageNameStr, parse_package_spec, read_package_json};
pub use oncemap::OnceMap;
pub use timing::{FETCH_TIMINGS, FetchTimings, FetchTimingsSnapshot};
