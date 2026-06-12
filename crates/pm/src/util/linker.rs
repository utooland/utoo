use std::fs as sfs;
use std::path::{self, Path, PathBuf};

use anyhow::{Context, Result};

use crate::fs;

/// Convert a path to absolute. Treats empty paths as `"."` because lockfiles
/// use `resolved: ""` for the root workspace linking itself.
fn to_absolute(p: &Path) -> Result<PathBuf> {
    let p = if p.as_os_str().is_empty() {
        Path::new(".")
    } else {
        p
    };
    path::absolute(p).context("Failed to resolve absolute path")
}

/// Resolve src/dst to absolute paths, ensure parent dir exists, clean up stale destination.
/// Returns `None` if the link is already up-to-date, `Some((src, dst))` if work is needed.
async fn prepare_link(src: &Path, dst: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    let abs_src = to_absolute(src)?;
    let abs_dst = to_absolute(dst)?;

    if !fs::try_exists(&abs_src).await? {
        anyhow::bail!("Source file does not exist: {}", abs_src.display());
    }

    if let Some(parent) = abs_dst.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    }

    if let Ok(metadata) = fs::symlink_metadata(&abs_dst).await {
        // Already a symlink pointing to the correct source — nothing to do.
        // Compare relative-to-relative to avoid `..` mismatch between lexicographic forms.
        if metadata.is_symlink()
            && let Ok(target) = fs::read_link(&abs_dst).await
            && let Some(parent) = abs_dst.parent()
            && pathdiff::diff_paths(&abs_src, parent).as_deref() == Some(&*target)
        {
            return Ok(None);
        }

        if metadata.is_dir() {
            fs::remove_dir_all(&abs_dst).await
        } else {
            fs::remove_file(&abs_dst).await
        }
        .with_context(|| format!("Failed to remove existing path: {}", abs_dst.display()))?;
    }

    Ok(Some((abs_src, abs_dst)))
}

/// Create a relative symlink: compute the path to `src` relative to `dst`'s
/// parent directory, then create the symlink.
async fn relative_symlink(src: &Path, dst: &Path) -> Result<()> {
    let parent = dst.parent().context("link destination has no parent")?;
    let rel = pathdiff::diff_paths(src, parent).context("Failed to compute relative path")?;
    symlink(&rel, dst).await
}

/// Create a relative symlink. Used for workspace links, `utoo link`, etc.
pub async fn link(src: &Path, dst: &Path) -> Result<()> {
    if let Some((abs_src, abs_dst)) = prepare_link(src, dst).await? {
        relative_symlink(&abs_src, &abs_dst).await?;
    }
    Ok(())
}

/// Sync sibling of [`prepare_link`] using `std::fs::*`.
///
/// The bin-link hot path runs thousands of cheap filesystem syscalls
/// (`symlink_metadata` + `symlink`). Routing each through `tokio::fs::*`
/// adds spawn_blocking + mpsc + thread-hop overhead that dominates the
/// real syscall cost — bench data showed ~6× slowdown for serial async
/// vs sync on ant-design (36ms → 6ms across 228 bins).
fn prepare_link_sync(src: &Path, dst: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    let abs_src = to_absolute(src)?;
    let abs_dst = to_absolute(dst)?;

    if !sfs::exists(&abs_src).context("Failed to probe source path")? {
        anyhow::bail!("Source file does not exist: {}", abs_src.display());
    }

    if let Some(parent) = abs_dst.parent() {
        sfs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    }

    if let Ok(metadata) = sfs::symlink_metadata(&abs_dst) {
        if metadata.is_symlink()
            && let Ok(target) = sfs::read_link(&abs_dst)
            && let Some(parent) = abs_dst.parent()
            && pathdiff::diff_paths(&abs_src, parent).as_deref() == Some(&*target)
        {
            return Ok(None);
        }

        if metadata.is_dir() {
            sfs::remove_dir_all(&abs_dst)
        } else {
            sfs::remove_file(&abs_dst)
        }
        .with_context(|| format!("Failed to remove existing path: {}", abs_dst.display()))?;
    }

    Ok(Some((abs_src, abs_dst)))
}

#[cfg(unix)]
fn relative_symlink_sync(src: &Path, dst: &Path) -> Result<()> {
    let parent = dst.parent().context("link destination has no parent")?;
    let rel = pathdiff::diff_paths(src, parent).context("Failed to compute relative path")?;
    std::os::unix::fs::symlink(&rel, dst).with_context(|| {
        format!(
            "Failed to create symbolic link from {} to {}",
            rel.display(),
            dst.display()
        )
    })
}

/// Link a binary into node_modules/.bin (synchronous).
///
/// - Unix: relative symlink so the link stays valid when the tree is mounted
///   at a different prefix (e.g. inside a Docker container).
/// - Windows: hardlink/copy + .cmd shim, following npm's cmd-shim convention.
///   Symlinks are avoided because they require admin privileges on Windows.
///
/// Sync because async tokio fs adds ~5× overhead for cheap bin-link syscalls;
/// the caller (`PackageService::execute_binary_linking`) drives this in a
/// `for` loop on the main task. Workspace symlinks still use the async
/// [`link`] above — that's a small handful of calls, not a hot path.
pub fn link_bin(src: &Path, dst: &Path) -> Result<()> {
    let Some((abs_src, abs_dst)) = prepare_link_sync(src, dst)? else {
        return Ok(());
    };

    #[cfg(unix)]
    relative_symlink_sync(&abs_src, &abs_dst)?;

    #[cfg(windows)]
    win_bin_shims_sync(&abs_src, &abs_dst)?;

    Ok(())
}

#[cfg(windows)]
fn win_bin_shims_sync(src: &Path, dst: &Path) -> Result<()> {
    let bin_dir = dst.parent().context("bin link has no parent dir")?;
    let bin_name = dst
        .file_name()
        .context("bin link has no file name")?
        .to_string_lossy();

    let rel =
        pathdiff::diff_paths(src, bin_dir).context("Failed to compute relative path for shim")?;
    let rel_str = rel.to_string_lossy();
    let rel_win = rel_str.replace('/', "\\");

    let is_native = src
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));

    if is_native {
        if sfs::hard_link(src, dst).is_err() {
            sfs::copy(src, dst).with_context(|| {
                format!(
                    "Failed to copy binary from {} to {}",
                    src.display(),
                    dst.display()
                )
            })?;
        }
        let exe_dst = dst.with_extension("exe");
        if sfs::hard_link(dst, &exe_dst).is_err() && sfs::copy(dst, &exe_dst).is_err() {
            tracing::warn!("Failed to create .exe alias at {}", exe_dst.display());
        }
    } else {
        let sh = format!(
            "#!/bin/sh\nbasedir=$(dirname \"$(echo \"$0\" | sed -e 's,\\\\,/,g')\")\n\"$basedir/{}\" \"$@\"\n",
            rel_str.replace('\\', "/")
        );
        sfs::write(dst, sh.as_bytes()).context("Failed to write sh shim")?;
    }

    let cmd_content = if is_native {
        format!("@\"%~dp0\\{rel_win}\" %*\r\n")
    } else {
        format!("@node \"%~dp0\\{rel_win}\" %*\r\n")
    };
    sfs::write(
        bin_dir.join(format!("{bin_name}.cmd")),
        cmd_content.as_bytes(),
    )
    .context("Failed to write .cmd shim")?;

    Ok(())
}

// ── Platform symlink ────────────────────────────────────────────────

#[cfg(unix)]
async fn symlink(src: &Path, dst: &Path) -> Result<()> {
    fs::symlink(src, dst).await.with_context(|| {
        format!(
            "Failed to create symbolic link from {} to {}",
            src.display(),
            dst.display()
        )
    })
}

#[cfg(windows)]
async fn symlink(src: &Path, dst: &Path) -> Result<()> {
    // `src` may be relative (e.g. `../foo`). Resolve against `dst`'s parent
    // so we can query metadata on the actual target.
    let resolved = if src.is_relative() {
        dst.parent()
            .map(|p| p.join(src))
            .unwrap_or(src.to_path_buf())
    } else {
        src.to_path_buf()
    };
    if fs::metadata(&resolved).await?.is_dir() {
        fs::symlink_dir(src, dst).await
    } else {
        fs::symlink_file(src, dst).await
    }
    .with_context(|| {
        format!(
            "Failed to create symbolic link from {} to {}",
            src.display(),
            dst.display()
        )
    })
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_link_creates_new_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_content = "test content";
        let src_path = temp_path.join("source1.txt");
        fs::write(&src_path, src_content).unwrap();

        let dst_path = temp_path.join("dest1.txt");

        assert!(!dst_path.exists());

        link(&src_path, &dst_path).await.unwrap();

        assert!(dst_path.exists());

        #[cfg(unix)]
        assert!(dst_path.is_symlink());

        assert_eq!(fs::read_to_string(&dst_path).unwrap(), src_content);
    }

    #[tokio::test]
    async fn test_link_creates_parent_directories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source2.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("nested/dir/dest2.txt");

        link(&src_path, &dst_path).await.unwrap();

        assert!(dst_path.exists());

        #[cfg(unix)]
        assert!(dst_path.is_symlink());
    }

    #[tokio::test]
    async fn test_link_existing_same_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source3.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("dest3.txt");

        link(&src_path, &dst_path).await.unwrap();
        let result = link(&src_path, &dst_path);
        assert!(result.await.is_ok());
    }

    #[tokio::test]
    async fn test_link_existing_different_target() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src1_path = temp_path.join("source4a.txt");
        let src2_path = temp_path.join("source4b.txt");
        fs::write(&src1_path, "test1").unwrap();
        fs::write(&src2_path, "test2").unwrap();

        let dst_path = temp_path.join("dest4.txt");

        link(&src1_path, &dst_path).await.unwrap();
        let result = link(&src2_path, &dst_path);
        assert!(result.await.is_ok());
        assert_eq!(fs::read_to_string(&dst_path).unwrap(), "test2");
    }

    #[cfg(unix)]
    #[test]
    fn test_link_bin_creates_relative_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Simulate node_modules/@napi-rs/cli/scripts/index.js
        let pkg_dir = temp_path.join("node_modules/@napi-rs/cli/scripts");
        fs::create_dir_all(&pkg_dir).unwrap();
        let src_path = pkg_dir.join("index.js");
        fs::write(&src_path, "#!/usr/bin/env node").unwrap();

        let dst_path = temp_path.join("node_modules/.bin/napi");

        link_bin(&src_path, &dst_path).unwrap();

        assert!(dst_path.is_symlink());
        // Symlink target must be relative, not absolute
        let raw_target = fs::read_link(&dst_path).unwrap();
        assert!(
            raw_target.is_relative(),
            "expected relative symlink, got: {}",
            raw_target.display()
        );
        assert_eq!(
            fs::read_to_string(&dst_path).unwrap(),
            "#!/usr/bin/env node"
        );

        // Calling link_bin again should be a no-op (idempotent)
        link_bin(&src_path, &dst_path).unwrap();
    }

    #[tokio::test]
    async fn test_link_replaces_existing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let src_path = temp_path.join("source5.txt");
        fs::write(&src_path, "test").unwrap();

        let dst_path = temp_path.join("dest5_dir");
        // Create a directory at destination
        fs::create_dir_all(&dst_path).unwrap();
        fs::write(dst_path.join("file.txt"), "content").unwrap();

        let result = link(&src_path, &dst_path).await;
        assert!(result.is_ok());

        #[cfg(unix)]
        assert!(dst_path.is_symlink());

        assert_eq!(fs::read_to_string(&dst_path).unwrap(), "test");
    }
}
