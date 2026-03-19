//! Platform-specific constants for cross-platform compatibility.

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
#[cfg(windows)]
pub fn drive_root(p: &std::path::Path) -> Option<std::ffi::OsString> {
    p.components()
        .next()
        .map(|c| c.as_os_str().to_ascii_uppercase())
}
