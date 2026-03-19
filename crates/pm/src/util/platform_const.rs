//! Platform-specific constants for cross-platform compatibility.

use std::ffi::OsString;
use std::path::Path;

/// PATH environment variable separator.
/// Windows uses `;`, Unix uses `:`.
pub const PATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

/// Global node_modules subdirectory relative to prefix.
/// Windows: `<prefix>/node_modules/<pkg>`
/// Unix: `<prefix>/lib/node_modules/<pkg>`
pub const GLOBAL_NODE_MODULES: &str = if cfg!(windows) {
    "node_modules"
} else {
    "lib/node_modules"
};

/// Extract the drive root component (e.g. `C:`) from a path, uppercased for comparison.
/// Returns `None` on Unix or if the path has no components.
pub fn drive_root(p: &Path) -> Option<OsString> {
    p.components()
        .next()
        .map(|c| c.as_os_str().to_ascii_uppercase())
}
