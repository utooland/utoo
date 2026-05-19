use anyhow::{Context, Result};
use bytes::Bytes;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MIN_ESTIMATED_SIZE: usize = 16;
const MAX_ESTIMATED_SIZE: usize = 512 * 1024 * 1024; // 512MB
const DECOMPRESSION_RETRY_FACTOR: usize = 4;

#[derive(Clone, Debug)]
pub(crate) struct ExtractedPackageIndex {
    pub(crate) root: PathBuf,
    pub(crate) dirs: Arc<[PathBuf]>,
    pub(crate) files: Arc<[PathBuf]>,
}

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

/// Synchronous extraction that also returns the file index observed while
/// parsing the tarball. Cold-install schedulers can reuse this index to
/// materialize immediately after extraction without walking the cache again.
pub(crate) fn extract_and_write_indexed_sync(
    gzip_bytes: Bytes,
    dest: &Path,
) -> Result<Option<ExtractedPackageIndex>> {
    let resolved_path = dest.join("_resolved");
    if resolved_path.try_exists()? {
        tracing::debug!("Extract skipped, already resolved: {}", dest.display());
        return Ok(None);
    }

    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    let estimated_size = estimate_uncompressed_size(&gzip_bytes);
    // The install scheduler already bounds package-level extract concurrency.
    // Reuse the bounded chunk writer so large tarballs do not monopolize one
    // extract worker while still avoiding per-file fan-out.
    extract_tarball_sync(gzip_bytes, estimated_size, dest)
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

/// Tarball entry recorded during the parse pass, deferred until parallel
/// write. `data` is a `bytes::Bytes` slice that shares the underlying
/// decompressed buffer via Arc, avoiding both the per-file `Vec<u8>`
/// allocation+copy that `read_to_end` previously did and the
/// offset/len arithmetic the previous zero-copy form needed at every
/// write call site.
struct ExtractedEntry {
    path: PathBuf,
    relative_path: PathBuf,
    data: Bytes,
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

    rx.await
        .with_context(|| "Extract task panicked")?
        .map(|_| ())
}

/// Synchronous extraction: decompress + parse + parallel write, all on rayon.
fn extract_tarball_sync(
    gzip_bytes: Bytes,
    estimated_size: usize,
    dest: &Path,
) -> Result<Option<ExtractedPackageIndex>> {
    use rayon::prelude::*;
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;

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

    // Move the decompressed buffer into `Bytes` so per-entry slices below
    // are reference-counted views into the same Arc-backed allocation —
    // no data copy, no offset arithmetic at the write call site.
    let buf = Bytes::from(output);

    // Parse tar entries — record `Bytes` slices into `buf` instead of
    // copying each file's bytes. The buffer outlives the parallel write
    // phase and is read-only after the parse loop, so concurrent slice
    // reads are safe.
    let entries = parse_tar_entries(&buf, dest)?;

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

    // Write files using par_chunks for batched parallelism. par_iter
    // spread every individual write across all rayon workers, queuing
    // N concurrent IO syscalls into the kernel scheduler at burst —
    // pcap+iostat A/B on GHA 2-core showed util_max=92% + w_await peaks
    // of 490ms paired with TCP-level retx=123 on the install hot path.
    // par_chunks(64) keeps cross-thread parallelism (each rayon worker
    // takes a 64-file chunk in parallel with sibling chunks) but bounds
    // in-flight write count per worker to 1. The same A/B with this
    // change collapsed retx 123 → 10, w_await 490ms → 160ms, util_max
    // 92% → 81%, with no observable wall-time loss vs par_iter (20s vs
    // 18s, within run-to-run noise).
    const WRITE_CHUNK_SIZE: usize = 64;
    let write_entry = |entry: &ExtractedEntry| -> Result<()> {
        let mut file = fs::File::create(&entry.path)
            .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
        file.write_all(&entry.data)
            .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

        // Skip chmod for 0o644 (most files) — File::create() already produces
        // this via umask (0o666 & ~0o022 = 0o644).
        #[cfg(unix)]
        if entry.mode != 0o644 {
            fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.mode)).ok();
        }
        Ok(())
    };

    entries
        .par_chunks(WRITE_CHUNK_SIZE)
        .try_for_each(|chunk| -> Result<()> {
            for entry in chunk {
                write_entry(entry)?;
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

    Ok(build_package_index(&entries))
}

fn build_package_index(entries: &[ExtractedEntry]) -> Option<ExtractedPackageIndex> {
    use std::collections::HashSet;
    use std::path::Component;

    let mut root = None::<PathBuf>;
    let mut dirs = HashSet::<PathBuf>::new();
    let mut seen_files = HashSet::<PathBuf>::new();
    let mut files = Vec::with_capacity(entries.len());

    for entry in entries {
        let entry_root = match entry.relative_path.components().next()? {
            Component::Normal(name) => PathBuf::from(name),
            _ => return None,
        };

        if let Some(root) = &root {
            if root != &entry_root {
                return None;
            }
        } else {
            root = Some(entry_root.clone());
        }

        let rel = entry
            .relative_path
            .strip_prefix(&entry_root)
            .ok()?
            .to_path_buf();
        if rel.as_os_str().is_empty() {
            return None;
        }

        if let Some(parent) = rel.parent() {
            let mut dir = PathBuf::new();
            for component in parent.components() {
                let Component::Normal(name) = component else {
                    return None;
                };
                dir.push(name);
                dirs.insert(dir.clone());
            }
        }
        if seen_files.insert(rel.clone()) {
            files.push(rel);
        }
    }

    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_unstable_by_key(|p| p.as_os_str().len());

    Some(ExtractedPackageIndex {
        root: root?,
        dirs: dirs.into(),
        files: files.into(),
    })
}

/// Walk the tar stream and collect file entries as `(path, offset, len, mode)`
/// tuples into the decompressed `buf`. Skips directories, ignores any
/// path-traversal entries.
fn parse_tar_entries(buf: &Bytes, dest: &Path) -> Result<Vec<ExtractedEntry>> {
    use std::io::Cursor;

    let cursor = Cursor::new(buf.as_ref());
    let mut archive = tar::Archive::new(cursor);
    let mut entries = Vec::new();

    for entry_result in archive.entries()? {
        let entry = entry_result.with_context(|| "Failed to read tar entry")?;
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

        if entry.header().entry_type().is_dir() {
            continue;
        }

        // tar::Entry exposes the byte offset and size inside the parsed
        // stream — the same `buf` we now reference-share via `Bytes`.
        // Use try_from rather than `as usize` so a malformed tar header
        // claiming a >usize::MAX size fails loudly instead of silently
        // truncating into a buffer-overrun bug on 32-bit targets.
        let data_offset = usize::try_from(entry.raw_file_position())
            .with_context(|| format!("Tar entry {} offset exceeds usize", path.display()))?;
        let data_len = usize::try_from(entry.size())
            .with_context(|| format!("Tar entry {} size exceeds usize", path.display()))?;
        if data_offset
            .checked_add(data_len)
            .is_none_or(|end| end > buf.len())
        {
            anyhow::bail!(
                "Tar entry {} extends past decompressed buffer ({}..+{} > {})",
                path.display(),
                data_offset,
                data_len,
                buf.len()
            );
        }

        let full_path = dest.join(&path);
        // Normalize to npm/pnpm convention: 0o755 if any exec bit is set,
        // else 0o644. Preserving raw tar modes (e.g. 0o640 in google-protobuf)
        // breaks world-readability in containers and multi-user setups.
        let raw_mode = entry.header().mode().unwrap_or(0o644);
        let mode = if raw_mode & 0o111 != 0 { 0o755 } else { 0o644 };
        entries.push(ExtractedEntry {
            path: full_path,
            relative_path: path,
            data: buf.slice(data_offset..data_offset + data_len),
            mode,
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> ExtractedEntry {
        ExtractedEntry {
            path: PathBuf::from("/cache").join(path),
            relative_path: PathBuf::from(path),
            data: Bytes::new(),
            mode: 0o644,
        }
    }

    #[test]
    fn package_index_records_paths_under_tarball_root() {
        let index = build_package_index(&[
            entry("package/package.json"),
            entry("package/dist/index.js"),
            entry("package/dist/nested/value.js"),
        ])
        .expect("index");

        assert_eq!(index.root, PathBuf::from("package"));
        assert_eq!(
            index.dirs.as_ref(),
            &[PathBuf::from("dist"), PathBuf::from("dist/nested")]
        );
        assert_eq!(
            index.files.as_ref(),
            &[
                PathBuf::from("package.json"),
                PathBuf::from("dist/index.js"),
                PathBuf::from("dist/nested/value.js"),
            ]
        );
    }

    #[test]
    fn package_index_rejects_multiple_tarball_roots() {
        let index = build_package_index(&[entry("package/a.js"), entry("other/b.js")]);
        assert!(index.is_none());
    }

    #[test]
    fn package_index_deduplicates_files_preserving_first_seen_order() {
        let index = build_package_index(&[
            entry("package/dist/index.js"),
            entry("package/package.json"),
            entry("package/dist/index.js"),
        ])
        .expect("index");

        assert_eq!(
            index.files.as_ref(),
            &[
                PathBuf::from("dist/index.js"),
                PathBuf::from("package.json")
            ]
        );
    }
}
