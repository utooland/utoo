//! Unified async filesystem operations.
//!
//! Re-exports tokio-fs-ext APIs and provides fallbacks for unsupported operations.
//! This module serves as a dispatch layer, making it easy to migrate to tokio-fs-ext
//! while maintaining fallbacks for APIs not yet supported.

// Re-export tokio-fs-ext APIs
pub use tokio_fs_ext::{
    // Metadata operations
    canonicalize,
    // Directory operations
    create_dir_all,
    metadata,
    read_dir,
    read_link,
    // File operations
    read_to_string,
    remove_dir_all,
    remove_file,
    rename,
    symlink_metadata,
    try_exists,
    write,
};

// Fallback to tokio::fs for operations not in tokio-fs-ext
pub use tokio::fs::{File, set_permissions};

// Unix-only symlink
#[cfg(unix)]
pub use tokio::fs::symlink;

// Windows-only symlink operations
#[cfg(windows)]
pub use tokio::fs::{symlink_dir, symlink_file};

// Test-only exports
#[cfg(test)]
pub use tokio_fs_ext::create_dir;
