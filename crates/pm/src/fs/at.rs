//! Directory-fd-based filesystem ops.
//!
//! All I/O inside a package tree works relative to an opened directory fd
//! instead of re-resolving absolute paths on every syscall. For a
//! `node_modules` with 100k+ files that cuts ~10 dentry lookups per file
//! per path down to 1 on each side.
//!
//! Available on Linux and macOS. Windows keeps the path-based API (no
//! `*at` equivalents). On macOS, cloning still uses `clonefile` — this
//! module is only wired into the extractor there.

use std::ffi::CStr;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

use rustix::fs::{AtFlags, Dir, Mode, OFlags, RawMode, Stat};

const DIR_OPEN_FLAGS: OFlags = OFlags::DIRECTORY
    .union(OFlags::RDONLY)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NOFOLLOW);

const FILE_CREATE_FLAGS: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::TRUNC)
    .union(OFlags::CLOEXEC);

/// Owned directory file descriptor. All methods operate relative to this fd.
pub struct DirFd {
    fd: OwnedFd,
}

// `link_from`, `stat`, and `read_entries` are only exercised by the Linux
// cloner (macOS uses `clonefile`). Silencing dead_code here instead of
// per-method `#[cfg]` gates keeps the API surface uniform.
#[allow(dead_code)]
impl DirFd {
    /// Open a directory by absolute path.
    pub fn open(path: &Path) -> io::Result<Self> {
        let fd = rustix::fs::open(path, DIR_OPEN_FLAGS, Mode::empty())?;
        Ok(Self { fd })
    }

    /// Open a subdirectory by name, relative to this dir fd.
    pub fn open_child(&self, name: &CStr) -> io::Result<Self> {
        let fd = rustix::fs::openat(&self.fd, name, DIR_OPEN_FLAGS, Mode::empty())?;
        Ok(Self { fd })
    }

    /// `mkdirat(self, name, mode)`. EEXIST is treated as success.
    ///
    /// `mode` is accepted as `u32` regardless of platform; the cast to
    /// `RawMode` (u32 on Linux, u16 on macOS) is a no-op for the values
    /// we pass (`0o644` / `0o755` fit comfortably in 16 bits).
    pub fn mkdir(&self, name: &CStr, mode: u32) -> io::Result<()> {
        match rustix::fs::mkdirat(&self.fd, name, Mode::from_raw_mode(mode as RawMode)) {
            Ok(()) => Ok(()),
            Err(e) if e == rustix::io::Errno::EXIST => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// `linkat(src, name, self, name, 0)`. Both oldpath and newpath are
    /// resolved relative to their dir fds — single-component lookup on
    /// each side.
    pub fn link_from(&self, src: &Self, name: &CStr) -> io::Result<()> {
        rustix::fs::linkat(&src.fd, name, &self.fd, name, AtFlags::empty())?;
        Ok(())
    }

    /// `openat(self, name, O_WRONLY | O_CREAT | O_TRUNC, mode)`. Returns
    /// an owned fd ready for `write`. Caller converts to `std::fs::File`
    /// if buffered I/O is needed.
    pub fn create_file(&self, name: &CStr, mode: u32) -> io::Result<OwnedFd> {
        let fd = rustix::fs::openat(
            &self.fd,
            name,
            FILE_CREATE_FLAGS,
            Mode::from_raw_mode(mode as RawMode),
        )?;
        Ok(fd)
    }

    /// `fstatat(self, name, AT_SYMLINK_NOFOLLOW)`.
    pub fn stat(&self, name: &CStr) -> io::Result<Stat> {
        Ok(rustix::fs::statat(
            &self.fd,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?)
    }

    /// Iterator over directory entries. Internally dups the fd so this
    /// `DirFd` remains usable for openat/linkat/mkdirat.
    pub fn read_entries(&self) -> io::Result<Dir> {
        Ok(Dir::read_from(&self.fd)?)
    }
}

impl AsFd for DirFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}
