//! Raw gzip/tar extraction for registry tarballs, built on the shared
//! primitives in [`utoo_ruborist::tar`] (gzip ISIZE sizing + libdeflate
//! decompression, the tar-slip path guard, the unified mode rule, and the
//! atomic cache-slot commit).
//!
//! # Durability contract
//!
//! Every cache slot becomes visible only via atomic rename of a
//! fully-written staging directory that already contains the `_resolved`
//! marker — the same protocol ruborist's `resolver/common.rs`
//! `commit_cache_dir_atomic` applies to git/http/file slots (and which this
//! module reuses). A `kill -9` at any point leaves either no slot or a
//! complete slot, never a partial tree a later run could mistake for a
//! cache hit.
//!
//! pm's value-add on top of the shared primitives is write throughput: the
//! decompressed buffer is wrapped in [`Bytes`] so per-entry payloads are
//! zero-copy slices, and file writes are fanned out on rayon in bounded
//! chunks.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bytes::Bytes;
use rayon::prelude::*;
use utoo_ruborist::tar::{
    commit_cache_dir_atomic, gzip_decompress, is_safe_tar_entry_path, normalize_entry_mode,
};

/// Extract gzip tarball bytes and atomically commit them to `dest`.
///
/// Skips if the `_resolved` marker already exists (warm cache).
pub async fn extract_and_write(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    // Check if already resolved (warm cache scenario)
    let resolved_path = dest.join("_resolved");
    if crate::fs::try_exists(&resolved_path).await? {
        tracing::debug!("Extract skipped, already resolved: {}", dest.display());
        return Ok(());
    }

    // A `dest` that exists *without* `_resolved` predates the staging
    // protocol (e.g. a slot partially written by an older pm killed
    // mid-extract). By the atomic-commit contract it can never become
    // valid on its own, so clear it before re-extracting — otherwise the
    // final rename would lose to the stale dir (ENOTEMPTY) and the slot
    // would stay broken forever.
    match crate::fs::remove_dir_all(dest).await {
        Ok(()) => {
            tracing::warn!(
                "Cleared stale cache slot without _resolved marker: {}",
                dest.display()
            );
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e)
                .with_context(|| format!("Failed to clear stale cache slot: {}", dest.display()));
        }
    }

    extract_tarball(gzip_bytes, dest).await
}

/// Tarball entry recorded during the parse pass, deferred until parallel
/// write. `data` is a `bytes::Bytes` slice that shares the underlying
/// decompressed buffer via Arc, avoiding both the per-file `Vec<u8>`
/// allocation+copy that `read_to_end` previously did and the
/// offset/len arithmetic the previous zero-copy form needed at every
/// write call site.
struct ExtractedEntry {
    path: PathBuf,
    data: Bytes,
    mode: u32,
}

/// Extract tarball using libdeflate for decompression + rayon for parallel writes.
///
/// Uses rayon::spawn (not tokio blocking pool) to avoid thread storms.
/// Rayon's global pool is configured with sufficient stack size at startup.
async fn extract_tarball(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    let dest_owned = dest.to_path_buf();

    let (tx, rx) = tokio::sync::oneshot::channel();

    rayon::spawn(move || {
        let result = extract_tarball_sync(gzip_bytes, &dest_owned);
        if tx.send(result).is_err() {
            tracing::warn!(
                "Extract result for {} discarded: receiver dropped",
                dest_owned.display()
            );
        }
    });

    rx.await.with_context(|| "Extract task panicked")?
}

/// Synchronous extraction: decompress, then stage + atomically commit, all
/// on rayon.
fn extract_tarball_sync(gzip_bytes: Bytes, dest: &Path) -> Result<()> {
    let decompressed = gzip_decompress(&gzip_bytes)
        .with_context(|| format!("Failed to decompress tarball for {}", dest.display()))?;

    // Move the decompressed buffer into `Bytes` so per-entry slices below
    // are reference-counted views into the same Arc-backed allocation —
    // no data copy, no offset arithmetic at the write call site.
    let buf = Bytes::from(decompressed);

    // Stage in a sibling temp dir and atomically rename into `dest`. The
    // shared helper writes `_resolved` into the staging dir *before* the
    // rename, and handles the rename-target-already-exists race (another
    // committer won; their complete slot is kept and ours is discarded).
    // See `ruborist/src/resolver/common.rs::commit_cache_dir_atomic`.
    commit_cache_dir_atomic(dest, |stage| write_entries_parallel(&buf, stage))
}

/// Parse the tar stream and write all file entries into the staging dir
/// using rayon for batched parallel writes.
fn write_entries_parallel(buf: &Bytes, stage: &Path) -> Result<()> {
    // Parse tar entries — record `Bytes` slices into `buf` instead of
    // copying each file's bytes. The buffer outlives the parallel write
    // phase and is read-only after the parse loop, so concurrent slice
    // reads are safe.
    let entries = parse_tar_entries(buf, stage)?;

    // Collect every ancestor directory (up to the staging root), then
    // create them shallowest-first so a single mkdir() per dir is
    // sufficient.
    let mut seen = HashSet::new();
    for entry in &entries {
        let mut p = entry.path.parent();
        while let Some(dir) = p {
            if dir == stage || !seen.insert(dir.to_path_buf()) {
                break;
            }
            p = dir.parent();
        }
    }
    let mut dirs: Vec<_> = seen.into_iter().collect();
    dirs.sort_unstable_by_key(|p| p.as_os_str().len());
    for dir in &dirs {
        if let Err(e) = fs::create_dir(dir)
            && e.kind() != ErrorKind::AlreadyExists
        {
            return Err(e)
                .with_context(|| format!("Failed to create directory: {}", dir.display()));
        }
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
    entries
        .par_chunks(WRITE_CHUNK_SIZE)
        .try_for_each(|chunk| -> Result<()> {
            for entry in chunk {
                let mut file = fs::File::create(&entry.path)
                    .with_context(|| format!("Failed to create: {}", entry.path.display()))?;
                file.write_all(&entry.data)
                    .with_context(|| format!("Failed to write: {}", entry.path.display()))?;

                // Skip chmod for 0o644 (most files) — File::create() already produces
                // this via umask (0o666 & ~0o022 = 0o644).
                #[cfg(unix)]
                if entry.mode != 0o644
                    && let Err(e) =
                        fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.mode))
                {
                    tracing::warn!(
                        "Failed to set mode {:o} on {}: {e}",
                        entry.mode,
                        entry.path.display()
                    );
                }
            }
            Ok(())
        })?;

    Ok(())
}

/// Walk the tar stream and collect file entries as zero-copy `Bytes` slices
/// into the decompressed `buf`, with paths joined under the staging dir.
/// Skips directories, ignores any path-traversal entries.
fn parse_tar_entries(buf: &Bytes, stage: &Path) -> Result<Vec<ExtractedEntry>> {
    let cursor = Cursor::new(buf.as_ref());
    let mut archive = tar::Archive::new(cursor);
    let mut entries = Vec::new();

    for entry_result in archive.entries()? {
        let entry = entry_result.with_context(|| "Failed to read tar entry")?;
        let path = entry
            .path()
            .with_context(|| "Failed to get entry path")?
            .into_owned();

        // Guard against path traversal (Tar Slip) — shared predicate so pm
        // and ruborist reject exactly the same entries.
        if !is_safe_tar_entry_path(&path) {
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

        let full_path = stage.join(&path);
        // Unified mode rule shared with ruborist's BFS-seeded slots:
        // preserve the archive's raw permission bits (setuid/setgid/sticky
        // stripped). See `utoo_ruborist::tar::normalize_entry_mode` for the
        // rationale.
        let mode = normalize_entry_mode(entry.header().mode().unwrap_or(0o644));
        entries.push(ExtractedEntry {
            path: full_path,
            data: buf.slice(data_offset..data_offset + data_len),
            mode,
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tempfile::TempDir;

    use super::*;

    fn gzip(data: &[u8]) -> Bytes {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        Bytes::from(encoder.finish().unwrap())
    }

    fn make_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            for (path, body) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(path).unwrap();
                header.set_size(body.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append(&header, *body).unwrap();
            }
            tar.finish().unwrap();
        }
        tar_data
    }

    #[tokio::test]
    async fn commits_complete_slot_with_resolved_marker() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("pkg").join("1.0.0");
        let bytes = gzip(&make_tar(&[("package/index.js", b"ok")]));

        extract_and_write(bytes, &dest).await.unwrap();
        assert!(dest.join("_resolved").exists());
        assert!(dest.join("package").join("index.js").exists());
    }

    /// The core staging guarantee: a partially-written staging dir never
    /// becomes a visible slot, and no staging leftovers survive a failure.
    #[tokio::test]
    async fn failed_extraction_never_publishes_a_slot() {
        let tmp = TempDir::new().unwrap();
        let parent = tmp.path().join("pkg");
        let dest = parent.join("1.0.0");

        // Truncate the tar stream mid-entry: the header claims more data
        // than the decompressed buffer holds, so the write phase fails
        // after the staging dir has been created.
        let tar = make_tar(&[("package/index.js", &[0u8; 4096])]);
        let truncated = gzip(&tar[..tar.len() / 2]);

        assert!(extract_and_write(truncated, &dest).await.is_err());
        assert!(!dest.exists());
        let leftovers = if parent.exists() {
            std::fs::read_dir(&parent).unwrap().count()
        } else {
            0
        };
        assert_eq!(leftovers, 0, "staging dir leaked into the cache parent");
    }

    /// A pre-staging-protocol slot (files present, no `_resolved`) is
    /// garbage by contract and must be replaced, not merged into.
    #[tokio::test]
    async fn stale_partial_slot_is_replaced() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("pkg").join("1.0.0");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("stale.js"), b"old").unwrap();

        let bytes = gzip(&make_tar(&[("package/index.js", b"new")]));
        extract_and_write(bytes, &dest).await.unwrap();

        assert!(dest.join("_resolved").exists());
        assert!(dest.join("package").join("index.js").exists());
        assert!(!dest.join("stale.js").exists());
    }
}
