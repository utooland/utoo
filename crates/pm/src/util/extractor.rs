use anyhow::{Context, Result};
use bytes::Bytes;
#[cfg(all(unix, not(target_os = "linux")))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const MIN_ESTIMATED_SIZE: usize = 16;
const MAX_ESTIMATED_SIZE: usize = 512 * 1024 * 1024; // 512MB
const DECOMPRESSION_RETRY_FACTOR: usize = 4;

/// Extract gzip tarball and write to destination.
///
/// Skips if `_resolved` marker already exists (warm cache).
pub async fn extract_and_write(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    // Check if already resolved (warm cache scenario)
    let resolved_path = dest.join("_resolved");
    if crate::fs::try_exists(&resolved_path).await? {
        tracing::debug!("Extract skipped, already resolved: {}", dest.display());
        return Ok(());
    }

    crate::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    extract_tarball(gzip_bytes, dest).await
}

/// Estimate uncompressed size from gzip footer (last 4 bytes store original size mod 2^32).
fn estimate_uncompressed_size(gzip_data: &[u8]) -> usize {
    if gzip_data.len() < 4 {
        return gzip_data.len() * 10; // fallback estimate
    }
    let last_4 = &gzip_data[gzip_data.len() - 4..];
    let size = u32::from_le_bytes([last_4[0], last_4[1], last_4[2], last_4[3]]) as usize;
    // Sanity check: if size is 0 or too small, use a reasonable estimate
    if !(MIN_ESTIMATED_SIZE..=MAX_ESTIMATED_SIZE).contains(&size) {
        gzip_data.len() * 10
    } else {
        size
    }
}

struct ExtractedEntry {
    /// Path relative to `dest`.
    rel_path: PathBuf,
    content: Vec<u8>,
    mode: u32,
}

/// Extract tarball using libdeflate for decompression + rayon for parallel writes.
///
/// Uses rayon::spawn (not tokio blocking pool) to avoid thread storms.
/// Rayon's global pool is configured with sufficient stack size at startup.
async fn extract_tarball(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    let estimated_size = estimate_uncompressed_size(&gzip_bytes);
    let dest_owned = dest.to_path_buf();

    let (tx, rx) = tokio::sync::oneshot::channel();

    rayon::spawn(move || {
        let result = extract_tarball_sync(gzip_bytes, estimated_size, &dest_owned);
        let _ = tx.send(result);
    });

    rx.await.with_context(|| "Extract task panicked")?
}

/// Synchronous extraction: decompress + parse + parallel write, all on rayon.
fn extract_tarball_sync(gzip_bytes: Bytes, estimated_size: usize, dest: &Path) -> Result<()> {
    use std::collections::HashSet;
    use std::io::{Cursor, Read};

    // Decompress gzip using libdeflate
    let mut output = vec![0u8; estimated_size];

    let mut decompressor = libdeflater::Decompressor::new();

    let actual_size = match decompressor.gzip_decompress(&gzip_bytes, &mut output) {
        Ok(size) => size,
        Err(libdeflater::DecompressionError::InsufficientSpace) => {
            let new_size = estimated_size * DECOMPRESSION_RETRY_FACTOR;
            output.resize(new_size, 0);
            decompressor
                .gzip_decompress(&gzip_bytes, &mut output)
                .with_context(|| "gzip decompression failed")?
        }
        Err(e) => return Err(anyhow::anyhow!("gzip decompression failed: {}", e)),
    };
    output.truncate(actual_size);

    // Parse tar entries
    let cursor = Cursor::new(&output[..]);
    let mut archive = tar::Archive::new(cursor);
    let mut entries = Vec::new();

    for entry_result in archive.entries()? {
        let mut entry = entry_result.with_context(|| "Failed to read tar entry")?;
        let path = entry
            .path()
            .with_context(|| "Failed to get entry path")?
            .into_owned();

        // Guard against path traversal (Tar Slip): reject absolute paths
        // and entries containing ".." components.
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            tracing::warn!("Skipping tar entry with unsafe path: {}", path.display());
            continue;
        }

        if !entry.header().entry_type().is_dir() {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("Failed to read tar entry: {}", path.display()))?;

            // Normalize to npm/pnpm convention: 0o755 if any exec bit is set,
            // else 0o644. Preserving raw tar modes (e.g. 0o640 in google-protobuf)
            // breaks world-readability in containers and multi-user setups.
            let raw_mode = entry.header().mode().unwrap_or(0o644);
            let mode = if raw_mode & 0o111 != 0 { 0o755 } else { 0o644 };
            entries.push(ExtractedEntry {
                rel_path: path,
                content,
                mode,
            });
        }
    }

    // Collect every ancestor directory relative to `dest`, shallowest-first.
    // Empty `PathBuf::new()` represents `dest` itself and is implicit
    // (already created by the caller).
    let mut seen = HashSet::new();
    for entry in &entries {
        let mut p = entry.rel_path.parent();
        while let Some(dir) = p {
            if dir.as_os_str().is_empty() || !seen.insert(dir.to_path_buf()) {
                break;
            }
            p = dir.parent();
        }
    }
    let mut rel_dirs: Vec<PathBuf> = seen.into_iter().collect();
    rel_dirs.sort_unstable_by_key(|p| p.as_os_str().len());

    write_entries(dest, &entries, &rel_dirs)
}

#[cfg(target_os = "linux")]
use write_via_dirfd as write_entries;

#[cfg(not(target_os = "linux"))]
use write_via_paths as write_entries;

#[cfg(not(target_os = "linux"))]
fn write_via_paths(dest: &Path, entries: &[ExtractedEntry], rel_dirs: &[PathBuf]) -> Result<()> {
    use rayon::prelude::*;
    use std::fs;
    use std::io::Write;

    for rel_dir in rel_dirs {
        fs::create_dir(dest.join(rel_dir)).ok();
    }

    entries.par_iter().try_for_each(|entry| -> Result<()> {
        let abs = dest.join(&entry.rel_path);
        let mut file = fs::File::create(&abs)
            .with_context(|| format!("Failed to create: {}", abs.display()))?;
        file.write_all(&entry.content)
            .with_context(|| format!("Failed to write: {}", abs.display()))?;

        // Skip chmod for 0o644 (most files) — File::create() already produces
        // this via umask (0o666 & ~0o022 = 0o644).
        #[cfg(unix)]
        if entry.mode != 0o644 {
            fs::set_permissions(&abs, fs::Permissions::from_mode(entry.mode)).ok();
        }
        Ok(())
    })?;

    #[cfg(unix)]
    {
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(dest, perms).ok();
    }

    fs::File::create(dest.join("_resolved"))
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

    Ok(())
}

/// Linux path: open every intermediate dir as a `DirFd`, then do parallel
/// `openat(parent_fd, leaf, O_WRONLY | O_CREAT | O_TRUNC, mode)` + write.
/// Each file write touches a single-component path — no absolute-path
/// dentry walk, no per-file `chmod` (mode is set at openat time, applied
/// through umask just like `File::create`).
#[cfg(target_os = "linux")]
fn write_via_dirfd(dest: &Path, entries: &[ExtractedEntry], rel_dirs: &[PathBuf]) -> Result<()> {
    use rayon::prelude::*;
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::Arc;

    use rustix::fs::{Mode, fchmod};

    use crate::util::at::DirFd;

    fn to_cstring(bytes: &[u8]) -> Result<CString> {
        CString::new(bytes).with_context(|| "tar entry name contains NUL byte")
    }

    let root_fd = Arc::new(
        DirFd::open(dest)
            .with_context(|| format!("Failed to open dest dir fd: {}", dest.display()))?,
    );

    let mut fds: HashMap<PathBuf, Arc<DirFd>> = HashMap::with_capacity(rel_dirs.len() + 1);
    fds.insert(PathBuf::new(), Arc::clone(&root_fd));

    for rel_dir in rel_dirs {
        let parent = rel_dir.parent().unwrap_or(Path::new(""));
        let parent_fd = fds
            .get(parent)
            .expect("rel_dirs is sorted shallow-first, parent already opened");
        let leaf = rel_dir
            .file_name()
            .expect("rel_dirs entries are never empty");
        let leaf_cstr = to_cstring(leaf.as_bytes())?;
        parent_fd
            .mkdir(&leaf_cstr, 0o755)
            .with_context(|| format!("mkdirat {}", rel_dir.display()))?;
        let new_fd = parent_fd
            .open_child(&leaf_cstr)
            .with_context(|| format!("openat {}", rel_dir.display()))?;
        fds.insert(rel_dir.clone(), Arc::new(new_fd));
    }

    let fds_ref = &fds;
    entries.par_iter().try_for_each(|entry| -> Result<()> {
        let parent = entry.rel_path.parent().unwrap_or(Path::new(""));
        let parent_fd = fds_ref
            .get(parent)
            .expect("all dirs pre-opened before parallel writes");
        let leaf = entry
            .rel_path
            .file_name()
            .expect("file entries have a leaf name");
        let leaf_cstr = to_cstring(leaf.as_bytes())?;

        let file_fd = parent_fd
            .create_file(&leaf_cstr, entry.mode)
            .with_context(|| format!("openat O_CREAT {}", entry.rel_path.display()))?;
        let mut file = std::fs::File::from(file_fd);
        file.write_all(&entry.content)
            .with_context(|| format!("Failed to write: {}", entry.rel_path.display()))?;
        Ok(())
    })?;

    // Ensure dest itself has 0o755 regardless of umask.
    let _ = fchmod(root_fd.as_ref(), Mode::from_raw_mode(0o755));

    let resolved_cstr = CString::new("_resolved").expect("static string has no NUL");
    drop(
        root_fd
            .create_file(&resolved_cstr, 0o644)
            .with_context(|| {
                format!("Failed to create resolution marker in: {}", dest.display())
            })?,
    );

    Ok(())
}
