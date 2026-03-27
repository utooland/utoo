use crate::fs;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Convert a path to absolute path
fn to_absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().context("Failed to get current working directory")?;
        Ok(cwd.join(path))
    }
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
        fs::create_dir_all(parent).await.context(format!(
            "Failed to create parent directory: {}",
            parent.display()
        ))?;
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
        .context(format!(
            "Failed to remove existing path: {}",
            abs_dst.display()
        ))?;
    }

    Ok(Some((abs_src, abs_dst)))
}

/// Create a relative symlink. Used for workspace links, `utoo link`, etc.
pub async fn link(src: &Path, dst: &Path) -> Result<()> {
    if let Some((abs_src, abs_dst)) = prepare_link(src, dst).await? {
        let parent = abs_dst.parent().context("link destination has no parent")?;
        let rel_src =
            pathdiff::diff_paths(&abs_src, parent).context("Failed to compute relative path")?;
        symlink(&rel_src, &abs_dst).await?;
    }
    Ok(())
}

/// Link a binary into node_modules/.bin.
///
/// - Unix: relative symlink so the link stays valid when the tree is mounted
///   at a different prefix (e.g. inside a Docker container).
/// - Windows: hardlink/copy + .cmd shim, following npm's cmd-shim convention.
///   Symlinks are avoided because they require admin privileges on Windows.
pub async fn link_bin(src: &Path, dst: &Path) -> Result<()> {
    let Some((abs_src, abs_dst)) = prepare_link(src, dst).await? else {
        return Ok(());
    };

    #[cfg(unix)]
    {
        let bin_dir = abs_dst.parent().context("bin link has no parent dir")?;
        let rel_src = pathdiff::diff_paths(&abs_src, bin_dir)
            .context("Failed to compute relative path for bin link")?;
        symlink(&rel_src, &abs_dst).await?;
    }

    #[cfg(windows)]
    win_bin_shims(&abs_src, &abs_dst).await?;

    Ok(())
}

// ── Platform symlink ────────────────────────────────────────────────

#[cfg(unix)]
async fn symlink(src: &Path, dst: &Path) -> Result<()> {
    fs::symlink(src, dst).await.context(format!(
        "Failed to create symbolic link from {} to {}",
        src.display(),
        dst.display()
    ))
}

#[cfg(windows)]
async fn symlink(src: &Path, dst: &Path) -> Result<()> {
    if fs::metadata(src).await?.is_dir() {
        fs::symlink_dir(src, dst).await
    } else {
        fs::symlink_file(src, dst).await
    }
    .context(format!(
        "Failed to create symbolic link from {} to {}",
        src.display(),
        dst.display()
    ))
}

// ── Windows bin shims ───────────────────────────────────────────────

/// Create Windows bin shims following npm's cmd-shim convention.
///
/// For each bin entry we create:
///   Native (.exe):  bare hardlink + .exe hardlink + .cmd shim
///   Script:         bare sh shim  + .cmd shim (with `node` prefix)
#[cfg(windows)]
async fn win_bin_shims(src: &Path, dst: &Path) -> Result<()> {
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
        win_link_native(src, dst).await?;
    } else {
        win_link_script(dst, &rel_str.replace('\\', "/")).await?;
    }

    // .cmd shim — always created
    let cmd_content = if is_native {
        format!("@\"%~dp0\\{rel_win}\" %*\r\n")
    } else {
        format!("@node \"%~dp0\\{rel_win}\" %*\r\n")
    };
    fs::write(
        bin_dir.join(format!("{bin_name}.cmd")),
        cmd_content.as_bytes(),
    )
    .await?;

    Ok(())
}

/// Hardlink a native PE binary as both bare name and .exe alias.
///
/// The bare name lets Git Bash execute it; the .exe alias is required
/// because Node.js `execFileSync` on Windows calls `CreateProcessW`
/// which only resolves `.exe` extensions.
#[cfg(windows)]
async fn win_link_native(src: &Path, dst: &Path) -> Result<()> {
    // bare name
    if fs::hard_link(src, dst).await.is_err() {
        fs::copy(src, dst).await.context(format!(
            "Failed to copy binary from {} to {}",
            src.display(),
            dst.display()
        ))?;
    }
    // .exe alias — link to bare name (already on disk) to avoid copying large binaries twice
    let exe_dst = dst.with_extension("exe");
    if fs::hard_link(dst, &exe_dst).await.is_err() && fs::copy(dst, &exe_dst).await.is_err() {
        tracing::warn!("Failed to create .exe alias at {}", exe_dst.display());
    }
    Ok(())
}

/// Write a shell shim for a script (e.g. `#!/usr/bin/env node`).
#[cfg(windows)]
async fn win_link_script(dst: &Path, rel_unix: &str) -> Result<()> {
    let sh = format!(
        "#!/bin/sh\nbasedir=$(dirname \"$(echo \"$0\" | sed -e 's,\\\\,/,g')\")\n\"$basedir/{rel_unix}\" \"$@\"\n"
    );
    fs::write(dst, sh.as_bytes()).await?;
    Ok(())
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
    #[tokio::test]
    async fn test_link_bin_creates_relative_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Simulate node_modules/@napi-rs/cli/scripts/index.js
        let pkg_dir = temp_path.join("node_modules/@napi-rs/cli/scripts");
        fs::create_dir_all(&pkg_dir).unwrap();
        let src_path = pkg_dir.join("index.js");
        fs::write(&src_path, "#!/usr/bin/env node").unwrap();

        let dst_path = temp_path.join("node_modules/.bin/napi");

        link_bin(&src_path, &dst_path).await.unwrap();

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
        link_bin(&src_path, &dst_path).await.unwrap();
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
