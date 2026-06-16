//! Unified async filesystem operations.
//!
//! Re-exports tokio-fs-ext APIs and provides fallbacks for unsupported operations.
//! This module serves as a dispatch layer, making it easy to migrate to tokio-fs-ext
//! while maintaining fallbacks for APIs not yet supported.

// Blanket allow because the consumer set of these re-exports is
// platform-conditional. The specific offenders: `hard_link` and `copy` are
// only called from the non-macOS clone fallback, `symlink` is unix-only,
// `symlink_dir`/`symlink_file` are windows-only — so on any single target a
// subset is "unused". If you remove a re-export's last caller on ALL
// platforms, delete the re-export too; this allow will not flag it for you.
#![allow(unused_imports)]

// Re-export tokio-fs-ext APIs
pub use tokio_fs_ext::{
    // Metadata operations
    canonicalize,
    // Directory operations
    copy,
    create_dir_all,
    metadata,
    // File operations
    read,
    read_dir,
    read_link,
    read_to_string,
    remove_dir,
    remove_dir_all,
    remove_file,
    rename,
    symlink_metadata,
    try_exists,
    write,
};

// Fallback to tokio::fs for operations not in tokio-fs-ext
pub use tokio::fs::{File, hard_link, set_permissions};

// Unix-only symlink
#[cfg(unix)]
pub use tokio::fs::symlink;

// Windows-only symlink operations
#[cfg(windows)]
pub use tokio::fs::{symlink_dir, symlink_file};

// Test-only exports
#[cfg(test)]
pub use tokio_fs_ext::create_dir;
