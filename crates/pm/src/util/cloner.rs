use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use utoo_ruborist::manifest::IdentityView;

use super::downloader::is_git_url;
use super::retry::create_retry_strategy;

/// How a cached package's real contents are laid out under its cache dir.
/// Derived from the resolved tarball URL so callers never pass a bare bool.
#[derive(Clone, Copy)]
enum CacheLayout {
    /// Registry tarball: contents live under a `package/` subdir, so the clone
    /// descends into the first real subdirectory.
    Wrapped,
    /// Git checkout: extracted flat at the cache root; clone the dir as-is.
    Flat,
}

impl CacheLayout {
    fn from_tarball_url(tarball_url: &str) -> Self {
        if is_git_url(tarball_url) {
            CacheLayout::Flat
        } else {
            CacheLayout::Wrapped
        }
    }
}

/// A request to materialize a cached package into a `node_modules` target.
///
/// Bundles the package identity, its resolved cache dir, and the destination so
/// the clone entry points take one coherent argument instead of five positional
/// ones, and new fields stay additive.
pub struct PackageClone<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub tarball_url: &'a str,
    pub cache: &'a Path,
    pub target: &'a Path,
}

#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "macos")]
use libc::clonefile;

/// Hardlink-first directory clone with a copy fallback. Used directly on
/// Linux/Windows, and on macOS as the fallback when `clonefile` can't run
/// (non-APFS volume or cross-device).
mod hardlink_clone {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::{fs, io};

    use anyhow::{Context, Result};

    struct CloneEntry {
        src: PathBuf,
        dst: PathBuf,
    }

    fn has_install_script_sync(src: &Path) -> bool {
        src.parent().is_some_and(|parent| {
            fs::metadata(parent.join("_hasInstallScript")).is_ok_and(|m| m.is_file())
        })
    }

    fn copy_file_sync(src: &Path, dst: &Path) -> io::Result<()> {
        fs::copy(src, dst)?;
        #[cfg(unix)]
        {
            let src_perms = fs::metadata(src)?.permissions();
            fs::set_permissions(dst, src_perms)?;
        }
        Ok(())
    }

    fn collect_entries(
        src: &Path,
        dst: &Path,
        files: &mut Vec<CloneEntry>,
        dirs: &mut Vec<PathBuf>,
    ) -> io::Result<()> {
        dirs.push(dst.to_path_buf());

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_name = entry.file_name();
            let target_path = dst.join(&file_name);

            if entry.file_type()?.is_dir() {
                collect_entries(&entry_path, &target_path, files, dirs)?;
            } else {
                files.push(CloneEntry {
                    src: entry_path,
                    dst: target_path,
                });
            }
        }

        Ok(())
    }

    /// Clone directory using sync I/O. Uses hardlink when possible, falls back
    /// to copy.
    pub fn clone_dir_sync(src: &Path, dst: &Path) -> Result<()> {
        let err_msg = format!("Failed to clone {} to {}", src.display(), dst.display());

        if !fs::metadata(src)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "Source is not a directory",
            ))
            .with_context(|| err_msg);
        }

        let mut force_copy = has_install_script_sync(src);

        let mut files = Vec::new();
        let mut dirs = Vec::new();
        collect_entries(src, dst, &mut files, &mut dirs)?;

        let mut created_dirs = HashSet::new();
        for dir in &dirs {
            if created_dirs.insert(dir.clone())
                && let Err(e) = fs::create_dir_all(dir)
                && e.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(e).with_context(|| err_msg.clone());
            }
        }

        let mut warned_per_file = false;
        for entry in &files {
            if force_copy {
                copy_file_sync(&entry.src, &entry.dst)?;
            } else if let Err(e) = fs::hard_link(&entry.src, &entry.dst) {
                if e.kind() == io::ErrorKind::CrossesDevices {
                    tracing::warn!(
                        "cross-device hardlink {} -> {}: {}; falling back to copy for remaining files",
                        src.display(),
                        dst.display(),
                        e
                    );
                    force_copy = true;
                } else if !warned_per_file {
                    tracing::warn!(
                        "hardlink failed for {} -> {}: {}; falling back to copy (further per-file failures suppressed)",
                        entry.src.display(),
                        entry.dst.display(),
                        e
                    );
                    warned_per_file = true;
                }
                copy_file_sync(&entry.src, &entry.dst)?;
            }
        }
        Ok(())
    }

    /// Async wrapper around [`clone_dir_sync`] for the dir-clone tests. The
    /// production path calls `clone_dir_sync` directly (in a blocking pool via
    /// the scheduler), so this wrapper is test-only. The tests that use it are
    /// macOS-excluded, so gate it the same way to avoid a dead-code warning.
    #[cfg(all(test, not(target_os = "macos")))]
    pub async fn clone_dir(src: &Path, dst: &Path) -> Result<()> {
        let src = src.to_path_buf();
        let dst = dst.to_path_buf();
        tokio::task::spawn_blocking(move || clone_dir_sync(&src, &dst)).await?
    }
}

fn load_package_json_sync<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let pkg_path = path.join("package.json");
    let content = std::fs::read_to_string(&pkg_path)
        .with_context(|| format!("Failed to read file {pkg_path:?}"))?;

    match serde_json::from_str(&content) {
        Ok(v) => Ok(v),
        Err(original_err) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(value) => serde_json::from_value(value)
                .with_context(|| format!("Failed to deserialize {pkg_path:?}")),
            Err(_) => {
                Err(original_err).with_context(|| format!("Failed to parse JSON from {pkg_path:?}"))
            }
        },
    }
}

fn validate_name_version_sync(dst: &Path, name: &str, version: &str) -> bool {
    let Ok(pkg) = load_package_json_sync::<IdentityView>(dst) else {
        return false;
    };
    pkg.name == name && pkg.version == version
}

fn find_real_src_sync(src: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(src).ok()? {
        let entry = entry.ok()?;
        if entry.file_type().ok()?.is_dir() {
            let path = entry.path();
            if path.file_name().is_some_and(|name| name != ".utoo_built") {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn clone_dir_native_sync(real_src: &Path, dst: &Path) -> Result<()> {
    let src_c = CString::new(real_src.as_os_str().as_bytes())?;
    let dst_c = CString::new(dst.as_os_str().as_bytes())?;
    let mut last_error = None;

    for delay in std::iter::once(std::time::Duration::ZERO).chain(create_retry_strategy()) {
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }

        match unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) } {
            0 => return Ok(()),
            _ => {
                let err = std::io::Error::last_os_error();
                let _ = std::fs::remove_dir_all(dst);
                let raw = err.raw_os_error();
                last_error = Some(err);
                // ENOTSUP (non-APFS volume) and EXDEV (cross-device) are
                // permanent: clonefile can never succeed here, so retrying only
                // adds delay to every clone. Fall back to hardlink/copy, which
                // hardlinks within a device and copies across one.
                if raw == Some(libc::ENOTSUP) || raw == Some(libc::EXDEV) {
                    return hardlink_clone::clone_dir_sync(real_src, dst);
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "clonefile {} -> {}: {}",
        real_src.display(),
        dst.display(),
        last_error
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    ))
}

#[cfg(not(target_os = "macos"))]
fn clone_dir_native_sync(real_src: &Path, dst: &Path) -> Result<()> {
    // Retry with backoff like the macOS arm: concurrent installs race on
    // create_dir_all/remove_dir_all of shared parents and can hit transient
    // EAGAIN/ENOENT during the hardlink walk. origin/next retried on all
    // platforms; the sync rewrite must keep that for Linux/Windows too.
    let mut last_error = None;
    for delay in std::iter::once(std::time::Duration::ZERO).chain(create_retry_strategy()) {
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        match hardlink_clone::clone_dir_sync(real_src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let _ = std::fs::remove_dir_all(dst);
                last_error = Some(e);
            }
        }
    }
    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("clone_dir failed without error"))
        .context(format!(
            "clone_dir {} -> {}",
            real_src.display(),
            dst.display()
        )))
}

fn clone_sync(src: &Path, dst: &Path, layout: CacheLayout) -> Result<()> {
    let real_src = match layout {
        CacheLayout::Wrapped => find_real_src_sync(src)
            .ok_or_else(|| anyhow::anyhow!("Cannot find valid source directory in {src:?}"))?,
        CacheLayout::Flat => src.to_path_buf(),
    };

    if dst.try_exists()?
        && let Err(e) = std::fs::remove_dir_all(dst)
    {
        tracing::warn!("Failed to clean target directory {}: {}", dst.display(), e);
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }

    clone_dir_native_sync(&real_src, dst)?;
    Ok(())
}

// find the first non built subdirectory

/// Sync clone from a resolved cache dir to `target_path`, validating the
/// existing target's name/version before skipping. The cache layout (registry
/// `package/` wrapper vs flat git checkout) is derived from `tarball_url`.
///
/// Returns `Ok(true)` when freshly materialized, `Ok(false)` when an existing
/// valid directory was reused. Stateless — callers (the install scheduler) own
/// dedup and counting.
pub fn clone_package_sync(req: &PackageClone<'_>) -> Result<bool> {
    if req.target.try_exists()? {
        if validate_name_version_sync(req.target, req.name, req.version) {
            return Ok(false);
        }
        if let Err(e) = std::fs::remove_dir_all(req.target) {
            tracing::warn!(
                "Failed to clean target directory {}: {}",
                req.target.display(),
                e
            );
        }
    }
    clone_sync(
        req.cache,
        req.target,
        CacheLayout::from_tarball_url(req.tarball_url),
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    #[cfg(not(target_os = "macos"))]
    use tokio::io::AsyncWriteExt;

    use super::*;
    #[cfg(not(target_os = "macos"))]
    use crate::fs;

    // Only consumed by `hardlink_clone_tests`, which is itself macOS-excluded.
    #[cfg(not(target_os = "macos"))]
    async fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> Result<PathBuf> {
        let path = dir.join(name);
        let mut file = fs::File::create(&path).await?;
        file.write_all(content).await?;
        Ok(path)
    }

    #[cfg(not(target_os = "macos"))]
    async fn create_test_structure(dir: &Path, structure: &[(&str, Option<&[u8]>)]) -> Result<()> {
        for (path, content) in structure {
            let full_path = dir.join(path);
            if let Some(content) = content {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                let mut file = fs::File::create(&full_path).await?;
                file.write_all(content).await?;
            } else {
                fs::create_dir_all(full_path).await?;
            }
        }
        Ok(())
    }

    fn create_package_json(name: &str, version: &str) -> String {
        format!(r#"{{"name": "{}", "version": "{}"}}"#, name, version)
    }

    #[test]
    fn test_clone_package_sync_fresh_install() -> Result<()> {
        let temp = TempDir::new()?;
        let cache_dir = temp.path().join("cache/lodash/4.17.21");
        let src_dir = cache_dir.join("package");
        let dst_dir = temp.path().join("node_modules/lodash");

        std::fs::create_dir_all(&src_dir)?;
        let pkg_json = create_package_json("lodash", "4.17.21");
        std::fs::write(src_dir.join("package.json"), &pkg_json)?;

        clone_package_sync(&PackageClone {
            name: "lodash",
            version: "4.17.21",
            tarball_url: "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
            cache: &cache_dir,
            target: &dst_dir,
        })?;

        assert!(dst_dir.join("package.json").exists());
        let content = std::fs::read_to_string(dst_dir.join("package.json"))?;
        assert!(content.contains("lodash"));
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    mod hardlink_clone_tests {
        use tokio::io::AsyncReadExt;

        use super::*;
        use crate::fs::File;

        /// Recursively compare two directories by entry set + file sizes
        /// (ignoring nested `node_modules`). Test-only assertion helper.
        async fn validate_directory(src: &Path, dst: &Path) -> Result<bool> {
            if !crate::fs::try_exists(dst).await? {
                return Ok(false);
            }
            if !fs::metadata(src).await?.is_dir() || !fs::metadata(dst).await?.is_dir() {
                return Ok(false);
            }

            #[derive(Debug)]
            struct EntryInfo {
                path: PathBuf,
                is_dir: bool,
                size: u64,
            }

            async fn collect_entries(
                dir: &Path,
                ignore: Option<&[&str]>,
            ) -> Result<Vec<EntryInfo>> {
                let mut entries = Vec::new();
                let mut read_dir = fs::read_dir(dir)
                    .await
                    .with_context(|| format!("Failed to read directory {}", dir.display()))?;
                while let Some(entry) = read_dir.next_entry().await? {
                    if let Some(ignore_list) = ignore
                        && let Some(file_name) = entry.path().file_name()
                        && ignore_list.contains(&&*file_name.to_string_lossy())
                    {
                        continue;
                    }
                    let metadata = entry.metadata().await.with_context(|| {
                        format!("Failed to get metadata for {}", entry.path().display())
                    })?;
                    entries.push(EntryInfo {
                        path: entry.path(),
                        is_dir: metadata.is_dir(),
                        size: if metadata.is_file() {
                            metadata.len()
                        } else {
                            0
                        },
                    });
                }
                Ok(entries)
            }

            let mut src_entries = collect_entries(src, Some(&["node_modules"])).await?;
            let mut dst_entries = collect_entries(dst, Some(&["node_modules"])).await?;
            src_entries.sort_by(|a, b| a.path.cmp(&b.path));
            dst_entries.sort_by(|a, b| a.path.cmp(&b.path));

            if src_entries.len() != dst_entries.len() {
                return Ok(false);
            }
            for (src_entry, dst_entry) in src_entries.iter().zip(dst_entries.iter()) {
                if src_entry.is_dir && dst_entry.is_dir {
                    if !Box::pin(validate_directory(&src_entry.path, &dst_entry.path)).await? {
                        return Ok(false);
                    }
                } else if !src_entry.is_dir && !dst_entry.is_dir {
                    if src_entry.size != dst_entry.size {
                        return Ok(false);
                    }
                } else {
                    return Ok(false);
                }
            }
            Ok(true)
        }

        #[tokio::test]
        async fn test_clone_dir_basic() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory structure
            create_test_structure(
                &src_dir,
                &[
                    ("file1.txt", Some(b"content1")),
                    ("file2.txt", Some(b"content2")),
                    ("subdir", None),
                    ("subdir/file3.txt", Some(b"content3")),
                ],
            )
            .await?;

            // Perform clone operation
            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify clone result
            assert!(validate_directory(&src_dir, &dst_dir).await?);

            // Verify file contents
            let mut content = String::new();
            File::open(dst_dir.join("file1.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "content1");

            content.clear();
            File::open(dst_dir.join("subdir/file3.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "content3");

            Ok(())
        }

        #[tokio::test]
        async fn test_clone_dir_nested() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create multi-level nested directory structure
            create_test_structure(
                &src_dir,
                &[
                    ("dir1", None),
                    ("dir1/dir2", None),
                    ("dir1/dir2/dir3", None),
                    ("dir1/dir2/dir3/file.txt", Some(b"deep content")),
                ],
            )
            .await?;

            // Perform clone operation
            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify clone result
            assert!(validate_directory(&src_dir, &dst_dir).await?);

            // Verify deep file content
            let mut content = String::new();
            File::open(dst_dir.join("dir1/dir2/dir3/file.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "deep content");

            Ok(())
        }

        #[tokio::test]
        async fn test_clone_dir_error_cases() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Test case when source directory doesn't exist
            let result = hardlink_clone::clone_dir(&src_dir, &dst_dir).await;
            assert!(result.is_err());
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );

            // Test case when source path is a file instead of a directory
            create_test_file(temp.path(), "not_a_dir", b"content").await?;
            let result =
                hardlink_clone::clone_dir(temp.path().join("not_a_dir").as_ref(), &dst_dir).await;
            assert!(result.is_err());

            Ok(())
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn test_fast_copy_preserves_permissions() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory with an executable file
            fs::create_dir(&src_dir).await?;
            let src_file = src_dir.join("executable.sh");
            fs::write(&src_file, b"#!/bin/bash\necho 'test'").await?;

            // Set executable permissions (0o755)
            let mut perms = fs::metadata(&src_file).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&src_file, perms).await?;

            // Verify source file has correct permissions
            let src_metadata = fs::metadata(&src_file).await?;
            assert_eq!(src_metadata.permissions().mode() & 0o777, 0o755);

            // Use clone_dir to trigger fast_copy_file path (no _hasInstallScript flag)
            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify destination file has same permissions
            let dst_file = dst_dir.join("executable.sh");
            let dst_metadata = fs::metadata(&dst_file).await?;
            assert_eq!(
                dst_metadata.permissions().mode() & 0o777,
                0o755,
                "Destination file should preserve executable permissions"
            );

            Ok(())
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn test_fast_copy_preserves_read_only_permissions() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory with a read-only file
            fs::create_dir(&src_dir).await?;
            let src_file = src_dir.join("readonly.txt");
            fs::write(&src_file, b"read only content").await?;

            // Set read-only permissions (0o444)
            let mut perms = fs::metadata(&src_file).await?.permissions();
            perms.set_mode(0o444);
            fs::set_permissions(&src_file, perms).await?;

            // Verify source file has correct permissions
            let src_metadata = fs::metadata(&src_file).await?;
            assert_eq!(src_metadata.permissions().mode() & 0o777, 0o444);

            // Use clone_dir to trigger fast_copy_file path
            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify destination file has same permissions
            let dst_file = dst_dir.join("readonly.txt");
            let dst_metadata = fs::metadata(&dst_file).await?;
            assert_eq!(
                dst_metadata.permissions().mode() & 0o777,
                0o444,
                "Destination file should preserve read-only permissions"
            );

            Ok(())
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn test_clone_dir_preserves_executable_in_subdirs() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create directory structure with executable files
            create_test_structure(
                &src_dir,
                &[
                    ("bin", None),
                    ("bin/script.sh", Some(b"#!/bin/bash\necho 'hello'")),
                    ("lib", None),
                    ("lib/data.txt", Some(b"data")),
                ],
            )
            .await?;

            // Set executable permission on script.sh
            let script_path = src_dir.join("bin/script.sh");
            let mut perms = fs::metadata(&script_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).await?;

            // Set read-only permission on data.txt
            let data_path = src_dir.join("lib/data.txt");
            let mut perms = fs::metadata(&data_path).await?.permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&data_path, perms).await?;

            // Clone directory
            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify script.sh has executable permissions
            let dst_script = dst_dir.join("bin/script.sh");
            let dst_script_metadata = fs::metadata(&dst_script).await?;
            assert_eq!(
                dst_script_metadata.permissions().mode() & 0o777,
                0o755,
                "Executable file should preserve permissions in subdirectories"
            );

            // Verify data.txt has correct permissions
            let dst_data = dst_dir.join("lib/data.txt");
            let dst_data_metadata = fs::metadata(&dst_data).await?;
            assert_eq!(
                dst_data_metadata.permissions().mode() & 0o777,
                0o644,
                "Regular file should preserve permissions in subdirectories"
            );

            Ok(())
        }

        /// Pre-populate the destination with a conflicting file so
        /// `fs::hard_link` fails with `AlreadyExists` (a non-EXDEV kind).
        /// The failing file must be copied (not hardlinked), and the
        /// remaining file must still be hardlinked — no global latch on
        /// per-file errors.
        #[cfg(unix)]
        #[tokio::test]
        async fn test_clone_dir_per_file_fallback_does_not_latch() -> Result<()> {
            use std::os::unix::fs::MetadataExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            create_test_structure(
                &src_dir,
                &[
                    ("file_a.txt", Some(b"content_a")),
                    ("file_b.txt", Some(b"content_b")),
                ],
            )
            .await?;

            // Pre-create a conflicting file_a so hard_link fails with AlreadyExists
            fs::create_dir_all(&dst_dir).await?;
            fs::write(dst_dir.join("file_a.txt"), b"stale").await?;

            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            assert_eq!(
                fs::read_to_string(dst_dir.join("file_a.txt")).await?,
                "content_a",
                "file_a should be overwritten by the copy fallback"
            );
            assert_eq!(
                fs::read_to_string(dst_dir.join("file_b.txt")).await?,
                "content_b"
            );

            // file_a: different inode → copied (fallback triggered)
            // file_b: same inode as src → hardlinked (no latch from file_a's failure)
            let src_a_ino = fs::metadata(src_dir.join("file_a.txt")).await?.ino();
            let dst_a_ino = fs::metadata(dst_dir.join("file_a.txt")).await?.ino();
            assert_ne!(
                src_a_ino, dst_a_ino,
                "file_a was copied, inode should differ"
            );

            let src_b_ino = fs::metadata(src_dir.join("file_b.txt")).await?.ino();
            let dst_b_ino = fs::metadata(dst_dir.join("file_b.txt")).await?.ino();
            assert_eq!(
                src_b_ino, dst_b_ino,
                "file_b should be hardlinked — per-file fallback must not latch"
            );

            Ok(())
        }

        /// When `_hasInstallScript` is set in the cache's parent, every
        /// file must be copied — hardlinks would let later install-script
        /// mutations leak back into the shared cache.
        #[cfg(unix)]
        #[tokio::test]
        async fn test_clone_dir_install_script_forces_copy() -> Result<()> {
            use std::os::unix::fs::MetadataExt;

            let temp = TempDir::new()?;
            // has_install_script_sync checks `src.parent()/_hasInstallScript`,
            // mirroring the cache layout `<cache>/<name>/<version>/package`.
            let cache_version = temp.path().join("pkg/1.0.0");
            let src_dir = cache_version.join("package");
            let dst_dir = temp.path().join("node_modules/pkg");

            fs::create_dir_all(&src_dir).await?;
            fs::write(src_dir.join("index.js"), b"module.exports = {}").await?;
            fs::write(cache_version.join("_hasInstallScript"), b"").await?;

            hardlink_clone::clone_dir(&src_dir, &dst_dir).await?;

            let src_ino = fs::metadata(src_dir.join("index.js")).await?.ino();
            let dst_ino = fs::metadata(dst_dir.join("index.js")).await?.ino();
            assert_ne!(
                src_ino, dst_ino,
                "install-script packages must be copied, not hardlinked"
            );
            Ok(())
        }
    }
}
