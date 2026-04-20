//! Unified filesystem operations.
//!
//! Re-exports tokio-fs-ext async APIs and provides fallbacks for unsupported
//! operations. Linux-only sync primitives (`DirFd` for `openat`/`linkat`-based
//! I/O) live in the [`at`] submodule.

// Some exports are only used on specific platforms (e.g., hard_link/copy on non-Unix)
#![allow(unused_imports)]

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod at;

// Re-export tokio-fs-ext APIs
pub use tokio_fs_ext::{
    // Metadata operations
    canonicalize,
    // Directory operations
    copy,
    create_dir_all,
    metadata,
    read_dir,
    read_link,
    // File operations
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
