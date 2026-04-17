use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use tokio_retry::Retry;
use utoo_ruborist::manifest::IdentityView;

use super::downloader::{is_git_url, resolve_cache_path};
use super::json::load_package_json;
use super::oncemap::OnceMap;
use super::retry::create_retry_strategy;
use crate::fs;

/// Global clone cache shared between pipeline and install phases.
///
/// Key: normalized target path. Install (`cwd.join("node_modules/foo")` →
/// forward slashes) and pipeline (`Path::join` injects backslashes on
/// Windows) produce the same logical target with different separators;
/// without normalization OnceMap sees them as distinct keys, dedup fails,
/// and concurrent tasks race on the same destination — manifesting as
/// `ERROR_SHARING_VIOLATION` (os error 32) on Windows. `PathBuf` from
/// `Path::components().collect()` parses both separators uniformly and
/// rebuilds with the OS-preferred one, giving a stable key.
static CLONE_CACHE: Lazy<OnceMap<PathBuf, ()>> = Lazy::new(OnceMap::new);

/// Number of clones completed.
static CLONE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Returns the number of fresh clones performed.
pub fn clone_count() -> usize {
    CLONE_COUNT.load(Ordering::Relaxed)
}

/// Normalize a target path into the canonical key used by `CLONE_CACHE`.
#[cfg(windows)]
fn cache_key(target_path: &Path) -> PathBuf {
    target_path.components().collect()
}

#[cfg(not(windows))]
fn cache_key(target_path: &Path) -> PathBuf {
    target_path.to_path_buf()
}

/// Wait for a pending clone at the given target path to complete (if any).
///
/// Used by the pipeline clone worker to ensure parent packages are
/// cloned before their children.
pub async fn wait_clone_if_pending(target_path: &str) {
    CLONE_CACHE
        .wait_if_pending(&cache_key(Path::new(target_path)))
        .await;
}

/// Clone a package to target path, downloading to cache first if needed.
///
/// Uses global `OnceMap` for deduplication: the same target path is only cloned once,
/// even when called concurrently from pipeline workers and the install phase.
pub async fn clone_package_once(
    name: &str,
    version: &str,
    tarball_url: &str,
    target_path: &Path,
) -> Result<()> {
    let key = cache_key(target_path);
    let err_label = format!("{name}@{version}");
    let name = name.to_string();
    let version = version.to_string();
    let tarball_url = tarball_url.to_string();
    let target_path = target_path.to_path_buf();

    // Git packages are extracted flat (no `package/` wrapper directory),
    // so skip `find_real_src` which would incorrectly pick a subdirectory.
    let is_git = is_git_url(&tarball_url);

    CLONE_CACHE
        .get_or_init(key, || async move {
            let cache_path = resolve_cache_path(&name, &version, &tarball_url).await?;
            clone_package(&cache_path, &target_path, &name, &version, !is_git)
                .await
                .inspect_err(|e| {
                    tracing::warn!(
                        "Clone failed: {}@{} to {}: {:#}",
                        name,
                        version,
                        target_path.display(),
                        e
                    )
                })
                .ok()?;

            CLONE_COUNT.fetch_add(1, Ordering::Relaxed);
            tracing::debug!("Cloned: {}@{} to {}", name, version, target_path.display());
            Some(())
        })
        .await
        .map(|_| ())
        .ok_or_else(|| anyhow::anyhow!("clone {err_label} failed (see warning log for details)"))
}

#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "macos")]
use libc::clonefile;

#[cfg(not(target_os = "macos"))]
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

    /// Clone directory using spawn_blocking for sync I/O.
    /// Uses hardlink when possible, falls back to copy.
    pub async fn clone_dir(src: &Path, dst: &Path) -> Result<()> {
        let err_msg = format!("Failed to clone {} to {}", src.display(), dst.display());
        let src = src.to_path_buf();
        let dst = dst.to_path_buf();

        tokio::task::spawn_blocking(move || {
            if !fs::metadata(&src)?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "Source is not a directory",
                ));
            }

            let use_copy = has_install_script_sync(&src);

            // Phase 1: Collect all files and directories
            let mut files = Vec::new();
            let mut dirs = Vec::new();
            collect_entries(&src, &dst, &mut files, &mut dirs)?;

            // Phase 2: Create all directories
            let mut created_dirs = HashSet::new();
            for dir in &dirs {
                if created_dirs.insert(dir.clone())
                    && let Err(e) = fs::create_dir_all(dir)
                    && e.kind() != io::ErrorKind::AlreadyExists
                {
                    return Err(e);
                }
            }

            // Phase 3: Clone files (hardlink or copy)
            for entry in &files {
                if use_copy {
                    copy_file_sync(&entry.src, &entry.dst)?;
                } else {
                    fs::hard_link(&entry.src, &entry.dst)?;
                }
            }
            Ok(())
        })
        .await?
        .with_context(|| err_msg)
    }
}

async fn validate_directory(src: &Path, dst: &Path) -> Result<bool> {
    if !crate::fs::try_exists(dst).await? {
        return Ok(false);
    }

    if !fs::metadata(src).await?.is_dir() || !fs::metadata(dst).await?.is_dir() {
        tracing::debug!("validating failed, since it's not a directory");
        return Ok(false);
    }

    #[derive(Debug)]
    struct EntryInfo {
        path: PathBuf,
        is_dir: bool,
        size: u64,
    }

    async fn collect_entries(dir: &Path, ignore: Option<&[&str]>) -> Result<Vec<EntryInfo>> {
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

    let mut src_entries = collect_entries(src, Some(&["node_modules"]))
        .await
        .with_context(|| format!("Failed to collect entries for {}", src.display()))?;
    let mut dst_entries = collect_entries(dst, Some(&["node_modules"]))
        .await
        .with_context(|| format!("Failed to collect entries for {}", dst.display()))?;

    src_entries.sort_by(|a, b| a.path.cmp(&b.path));
    dst_entries.sort_by(|a, b| a.path.cmp(&b.path));

    if src_entries.len() != dst_entries.len() {
        tracing::debug!(
            "validating failed {}:{} to {}:{}, since entries length is not equal\nsrc entries: {:?}\ndst entries: {:?}",
            src.display(),
            src_entries.len(),
            dst.display(),
            dst_entries.len(),
            src_entries
                .iter()
                .map(|e| e.path.file_name().unwrap_or_default())
                .collect::<Vec<_>>(),
            dst_entries
                .iter()
                .map(|e| e.path.file_name().unwrap_or_default())
                .collect::<Vec<_>>()
        );
        return Ok(false);
    }

    for (src_entry, dst_entry) in src_entries.iter().zip(dst_entries.iter()) {
        if src_entry.is_dir && dst_entry.is_dir {
            let future = validate_directory(&src_entry.path, &dst_entry.path);
            if !Box::pin(future).await? {
                return Ok(false);
            }
        } else if !src_entry.is_dir && !dst_entry.is_dir {
            if src_entry.size != dst_entry.size {
                tracing::debug!(
                    "validating failed {}:{} to {}:{}, since diff size",
                    src_entry.path.display(),
                    src_entry.size,
                    dst_entry.path.display(),
                    dst_entry.size
                );
                return Ok(false);
            }
        } else {
            tracing::debug!(
                "validating failed {}:{} to {}:{}, since diff size",
                src_entry.path.display(),
                src_entry.size,
                dst_entry.path.display(),
                dst_entry.size
            );
            return Ok(false);
        }
    }

    Ok(true)
}

// find the first non built subdirectory
pub async fn find_real_src<P: AsRef<Path>>(src: P) -> Option<PathBuf> {
    let mut read_dir = fs::read_dir(src.as_ref()).await.ok()?;
    while let Some(entry) = read_dir.next_entry().await.ok()? {
        if let Ok(metadata) = entry.metadata().await
            && metadata.is_dir()
            && let Some(name) = entry.path().file_name()
            && name.to_string_lossy() != ".utoo_built"
        {
            return Some(entry.path());
        }
    }
    None
}

async fn clone(src: &Path, dst: &Path, find_real: bool) -> Result<()> {
    let real_src = if find_real {
        find_real_src(src)
            .await
            .ok_or_else(|| anyhow::anyhow!("Cannot find valid source directory in {src:?}"))?
    } else {
        src.to_path_buf()
    };

    if crate::fs::try_exists(dst).await? {
        let is_valid = validate_directory(&real_src, dst)
            .await
            .unwrap_or_else(|e| {
                tracing::debug!("validate_directory error: {e}, will override target directory");
                false
            });

        if is_valid {
            tracing::debug!(
                "Target directory {} already exists and validation passed, skipping clone",
                dst.display()
            );
            return Ok(());
        }

        tracing::debug!("{real_src:?} --> {dst:?} overrides");
        if let Err(e) = fs::remove_dir_all(dst).await {
            tracing::warn!("Failed to clean target directory {}: {}", dst.display(), e);
        }
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).await?;
    }

    #[cfg(target_os = "macos")]
    {
        let src_c = CString::new(real_src.as_os_str().as_bytes())?;
        let dst_c = CString::new(dst.as_os_str().as_bytes())?;

        Retry::spawn(create_retry_strategy(), || async {
            match unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) } {
                0 => {
                    tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
                    Ok(())
                }
                _ => {
                    let _ = fs::remove_dir_all(dst).await.map_err(|e| {
                        tracing::debug!(
                            "Failed to clean target directory {}: {}",
                            dst.display(),
                            e
                        );
                    });
                    Err(anyhow::anyhow!(
                        "Failed to clone file: {}",
                        std::io::Error::last_os_error()
                    ))
                }
            }
        })
        .await?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        Retry::spawn(create_retry_strategy(), || async {
            hardlink_clone::clone_dir(&real_src, dst).await?;
            tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    }

    Ok(())
}

/// Validate that the package.json in dst has matching name and version
async fn validate_name_version(dst: &Path, name: &str, version: &str) -> bool {
    let Ok(pkg) = load_package_json::<IdentityView>(dst).await else {
        return false;
    };
    pkg.name == name && pkg.version == version
}

/// Clone a package from cache to destination with name/version validation.
///
/// `find_real`: if `true`, look for the first subdirectory in `src` (registry
/// tarballs use a `package/` wrapper); if `false`, use `src` directly (git
/// packages are extracted flat).
pub async fn clone_package(
    src: &Path,
    dst: &Path,
    name: &str,
    version: &str,
    find_real: bool,
) -> Result<()> {
    match crate::fs::try_exists(dst).await? {
        true if validate_name_version(dst, name, version).await => {
            tracing::debug!(
                "Package {}@{} already exists at {}, skipping clone",
                name,
                version,
                dst.display()
            );
            Ok(())
        }
        true => {
            tracing::debug!(
                "Package at {} has mismatched name/version, removing and re-cloning",
                dst.display()
            );
            if let Err(e) = fs::remove_dir_all(dst).await {
                tracing::warn!("Failed to clean target directory {}: {}", dst.display(), e);
            }
            clone(src, dst, find_real).await
        }
        false => clone(src, dst, find_real).await,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[cfg(windows)]
    #[test]
    fn cache_key_normalizes_path_separators() {
        // install.rs joins lockfile-derived strings (forward slashes) while
        // pipeline workers go through `Path::join` (backslashes). Both must
        // produce the same OnceMap key — otherwise concurrent clones race
        // and Windows raises ERROR_SHARING_VIOLATION.
        let forward = cache_key(Path::new("node_modules/@scope/pkg/node_modules/dep"));
        let backward = cache_key(Path::new("node_modules\\@scope\\pkg\\node_modules\\dep"));
        let mixed = cache_key(Path::new("node_modules/@scope/pkg\\node_modules\\dep"));
        assert_eq!(forward, backward);
        assert_eq!(forward, mixed);
    }

    async fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> Result<PathBuf> {
        let path = dir.join(name);
        let mut file = fs::File::create(&path).await?;
        file.write_all(content).await?;
        Ok(path)
    }

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

    #[tokio::test]
    async fn test_validate_directory_different_sizes() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(&src_dir, &[("file.txt", Some(b"content1"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"different"))]).await?;

        assert!(!validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_same_content() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(&src_dir, &[("file.txt", Some(b"same content"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"same content"))]).await?;

        assert!(validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_nested_structure() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(
            &src_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir1/dir2/file2.txt", Some(b"content2")),
                ("dir3/file3.txt", Some(b"content3")),
            ],
        )
        .await?;

        create_test_structure(
            &dst_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir1/dir2/file2.txt", Some(b"content2")),
                ("dir3/file3.txt", Some(b"content3")),
            ],
        )
        .await?;

        assert!(validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_different_structure() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(
            &src_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir2/file2.txt", Some(b"content2")),
            ],
        )
        .await?;

        create_test_structure(
            &dst_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir3/file3.txt", Some(b"content31")),
            ],
        )
        .await?;

        assert!(!validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_find_real_src() -> Result<()> {
        let temp = TempDir::new()?;
        let dir = temp.path().join("test_dir");
        fs::create_dir(&dir).await?;

        assert!(find_real_src(&dir).await.is_none());

        create_test_file(&dir, "file.txt", b"content").await?;
        assert!(find_real_src(&dir).await.is_none());

        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).await?;
        assert_eq!(find_real_src(&dir).await.unwrap(), subdir);

        Ok(())
    }

    #[tokio::test]
    async fn test_find_real_src_with_built_dir() -> Result<()> {
        let temp = TempDir::new()?;
        let dir = temp.path().join("test_dir");
        fs::create_dir(&dir).await?;

        // Create .utoo_built directory
        let built_dir = dir.join(".utoo_built");
        fs::create_dir(&built_dir).await?;

        // Create a regular subdirectory
        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).await?;

        assert_eq!(find_real_src(&dir).await.unwrap(), subdir);
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_without_find_real() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        // Create source structure
        create_test_structure(
            &src_dir,
            &[
                (".utoo_built", None),
                ("real_dir/file.txt", Some(b"content")),
            ],
        )
        .await?;

        // Test cloning with find_real=false
        clone(&src_dir, &dst_dir, false).await?;

        // Verify everything was cloned
        assert!(dst_dir.join("real_dir").exists());
        assert!(dst_dir.join(".utoo_built").exists());
        assert_eq!(
            fs::read_to_string(dst_dir.join("real_dir/file.txt")).await?,
            "content"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_clone_existing_directory() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        // Create initial source and destination
        create_test_structure(&src_dir, &[("file.txt", Some(b"old content"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"old content"))]).await?;

        // Clone should succeed and skip since content is identical
        clone(&src_dir, &dst_dir, false).await?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("file.txt")).await?,
            "old content"
        );

        // Update source content
        create_test_structure(&src_dir, &[("file.txt", Some(b"content changed"))]).await?;

        // Clone should update the destination
        clone(&src_dir, &dst_dir, false).await?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("file.txt")).await?,
            "content changed"
        );

        Ok(())
    }

    fn create_package_json(name: &str, version: &str) -> String {
        format!(r#"{{"name": "{}", "version": "{}"}}"#, name, version)
    }

    #[tokio::test]
    async fn test_validate_name_version_matching() -> Result<()> {
        let temp = TempDir::new()?;
        let pkg_dir = temp.path().join("pkg");
        fs::create_dir_all(&pkg_dir).await?;

        let pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(pkg_dir.join("package.json"), pkg_json).await?;

        assert!(validate_name_version(&pkg_dir, "lodash", "4.17.21").await);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_name_version_name_mismatch() -> Result<()> {
        let temp = TempDir::new()?;
        let pkg_dir = temp.path().join("pkg");
        fs::create_dir_all(&pkg_dir).await?;

        let pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(pkg_dir.join("package.json"), pkg_json).await?;

        assert!(!validate_name_version(&pkg_dir, "underscore", "4.17.21").await);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_name_version_version_mismatch() -> Result<()> {
        let temp = TempDir::new()?;
        let pkg_dir = temp.path().join("pkg");
        fs::create_dir_all(&pkg_dir).await?;

        let pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(pkg_dir.join("package.json"), pkg_json).await?;

        assert!(!validate_name_version(&pkg_dir, "lodash", "4.17.20").await);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_name_version_no_package_json() -> Result<()> {
        let temp = TempDir::new()?;
        let pkg_dir = temp.path().join("pkg");
        fs::create_dir_all(&pkg_dir).await?;

        // No package.json file
        assert!(!validate_name_version(&pkg_dir, "lodash", "4.17.21").await);
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_package_skip_if_valid() -> Result<()> {
        let temp = TempDir::new()?;
        // Cache structure: cache_dir/package/ (find_real looks for first subdir)
        let cache_dir = temp.path().join("cache/lodash/4.17.21");
        let src_dir = cache_dir.join("package");
        let dst_dir = temp.path().join("node_modules/lodash");

        // Create source (with subdir structure that find_real expects)
        fs::create_dir_all(&src_dir).await?;
        let pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(src_dir.join("package.json"), &pkg_json).await?;
        fs::write(src_dir.join("index.js"), "module.exports = {}").await?;

        // Create destination with same content
        fs::create_dir_all(&dst_dir).await?;
        fs::write(dst_dir.join("package.json"), &pkg_json).await?;
        fs::write(dst_dir.join("index.js"), "module.exports = {}").await?;

        // Add a marker file to verify it wasn't re-cloned
        fs::write(dst_dir.join("marker.txt"), "original").await?;

        clone_package(&cache_dir, &dst_dir, "lodash", "4.17.21", true).await?;

        // Marker file should still exist (wasn't deleted and re-cloned)
        assert!(dst_dir.join("marker.txt").exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_package_reclone_if_version_mismatch() -> Result<()> {
        let temp = TempDir::new()?;
        let cache_dir = temp.path().join("cache/lodash/4.17.21");
        let src_dir = cache_dir.join("package");
        let dst_dir = temp.path().join("node_modules/lodash");

        // Create source with new version
        fs::create_dir_all(&src_dir).await?;
        let new_pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(src_dir.join("package.json"), &new_pkg_json).await?;

        // Create destination with old version
        fs::create_dir_all(&dst_dir).await?;
        let old_pkg_json = create_package_json("lodash", "4.17.20");
        fs::write(dst_dir.join("package.json"), &old_pkg_json).await?;
        fs::write(dst_dir.join("marker.txt"), "should be deleted").await?;

        clone_package(&cache_dir, &dst_dir, "lodash", "4.17.21", true).await?;

        // Marker file should be gone (directory was deleted and re-cloned)
        assert!(!dst_dir.join("marker.txt").exists());
        // New package.json should have correct version
        let content = fs::read_to_string(dst_dir.join("package.json")).await?;
        assert!(content.contains("4.17.21"));
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_package_fresh_install() -> Result<()> {
        let temp = TempDir::new()?;
        let cache_dir = temp.path().join("cache/lodash/4.17.21");
        let src_dir = cache_dir.join("package");
        let dst_dir = temp.path().join("node_modules/lodash");

        // Create source
        fs::create_dir_all(&src_dir).await?;
        let pkg_json = create_package_json("lodash", "4.17.21");
        fs::write(src_dir.join("package.json"), &pkg_json).await?;

        // Destination doesn't exist
        assert!(!dst_dir.exists());

        clone_package(&cache_dir, &dst_dir, "lodash", "4.17.21", true).await?;

        // Should be cloned
        assert!(dst_dir.join("package.json").exists());
        let content = fs::read_to_string(dst_dir.join("package.json")).await?;
        assert!(content.contains("lodash"));
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_package_git_flat_layout() -> Result<()> {
        let temp = TempDir::new()?;
        // Git packages are extracted flat — package.json is at the root,
        // not inside a `package/` subdirectory.
        let cache_dir = temp.path().join("cache/my-git-pkg/abc123");
        let dst_dir = temp.path().join("node_modules/my-git-pkg");

        // Create source with flat layout (no package/ wrapper)
        fs::create_dir_all(&cache_dir).await?;
        let pkg_json = create_package_json("my-git-pkg", "1.0.0");
        fs::write(cache_dir.join("package.json"), &pkg_json).await?;
        fs::write(cache_dir.join("index.js"), "module.exports = {}").await?;

        clone_package(&cache_dir, &dst_dir, "my-git-pkg", "1.0.0", false).await?;

        // Should clone directly from cache root (not looking for package/ subdir)
        assert!(dst_dir.join("package.json").exists());
        assert!(dst_dir.join("index.js").exists());
        let content = fs::read_to_string(dst_dir.join("package.json")).await?;
        assert!(content.contains("my-git-pkg"));
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    mod hardlink_clone_tests {
        use tokio::io::AsyncReadExt;

        use super::*;
        use crate::fs::File;

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
    }
}
