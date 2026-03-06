use anyhow::{Context, Result};
use bytes::Bytes;
#[cfg(unix)]
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
    path: PathBuf,
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
    use rayon::prelude::*;
    use std::collections::HashSet;
    use std::fs;
    use std::io::{Cursor, Read, Write};

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

        let full_path = dest.join(&path);

        if !entry.header().entry_type().is_dir() {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("Failed to read tar entry: {}", path.display()))?;

            let mode = entry.header().mode().unwrap_or(0o644);
            entries.push(ExtractedEntry {
                path: full_path,
                content,
                mode,
            });
        }
    }

    // Collect every ancestor directory (up to `dest`), then create them
    // shallowest-first so a single mkdir() per dir is sufficient.
    let mut seen = HashSet::new();
    for entry in &entries {
        let mut p = entry.path.parent();
        while let Some(dir) = p {
            if dir == dest || !seen.insert(dir.to_path_buf()) {
                break;
            }
            p = dir.parent();
        }
    }
    let mut dirs: Vec<_> = seen.into_iter().collect();
    dirs.sort_unstable_by_key(|p| p.as_os_str().len());
    for dir in &dirs {
        fs::create_dir(dir).ok();
    }

    // Write files in parallel using rayon
    entries.par_iter().try_for_each(|entry| -> Result<()> {
        let mut file = fs::File::create(&entry.path)
            .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
        file.write_all(&entry.content)
            .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

        // Skip chmod for 0o644 (most files) — File::create() already produces
        // this via umask (0o666 & ~0o022 = 0o644).
        #[cfg(unix)]
        if entry.mode != 0o644 {
            fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.mode)).ok();
        }
        Ok(())
    })?;

    // Set directory permissions
    #[cfg(unix)]
    {
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(dest, perms).ok();
    }

    // Create resolution marker
    fs::File::create(dest.join("_resolved"))
        .with_context(|| format!("Failed to create resolution marker in: {}", dest.display()))?;

    Ok(())
}
