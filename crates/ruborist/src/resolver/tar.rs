//! Tar + gzip helpers shared by the HTTP and file tarball resolvers.
//!
//! Both resolvers need to parse an npm-style `package/`-prefixed tarball
//! into in-memory [`TarEntry`]s (so the manifest can be inspected before
//! deciding where to commit) and then emit those entries atomically into
//! a cache slot.
//!
//! The low-level primitives — [`estimate_uncompressed_size`],
//! [`gzip_decompress`], [`is_safe_tar_entry_path`], and
//! [`normalize_entry_mode`] — are also consumed by pm's install-phase
//! extractor (`crates/pm/src/util/extractor.rs`) via the
//! [`crate::tar`] façade, so registry slots and BFS-seeded slots are
//! produced by the exact same gzip/tar rules.

use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use super::common::{commit_cache_dir_atomic, finalize_non_registry_manifest};
use crate::model::manifest::CoreVersionManifest;

/// Decoded tar entry held in memory until the package name/version is known
/// and the final cache path can be resolved.
#[derive(Debug)]
pub(crate) struct TarEntry {
    pub rel_path: PathBuf,
    pub content: Vec<u8>,
    pub mode: u32,
    pub is_dir: bool,
}

/// Sanity ceiling for gzip ISIZE trust and for `file:` directory copies —
/// keeps both paths aligned on the same "biggest reasonable package" limit.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;

/// Single-pass tar scan: collects safe entries **and** the shallowest
/// `package.json` blob for manifest extraction.
///
/// npm tarballs put everything under a top-level `package/` dir, so the
/// `package.json` at depth 2 is canonical. Deeper matches win only as
/// fallback for non-standard layouts.
pub(crate) fn scan_tarball(tar_bytes: &[u8]) -> Result<(Vec<TarEntry>, Vec<u8>)> {
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    archive.set_preserve_permissions(false);

    let mut entries = Vec::new();
    let mut manifest_blob: Option<(usize, Vec<u8>)> = None;

    for entry_result in archive.entries().context("failed to iterate tar entries")? {
        let mut entry = entry_result.context("failed to read tar entry")?;
        let rel_path = entry
            .path()
            .context("failed to read tar entry path")?
            .into_owned();

        if !is_safe_tar_entry_path(&rel_path) {
            tracing::warn!(
                "Skipping tar entry with unsafe path: {}",
                rel_path.display()
            );
            continue;
        }

        let is_dir = entry.header().entry_type().is_dir();
        let mode = normalize_entry_mode(entry.header().mode().unwrap_or(0o644));
        // Don't pre-reserve from entry.size() — a crafted header could trigger
        // huge allocations. Let Vec grow naturally.
        let mut content = Vec::new();
        if !is_dir {
            entry
                .read_to_end(&mut content)
                .with_context(|| format!("failed to read tar entry {}", rel_path.display()))?;
        }

        if !is_dir && rel_path.file_name().is_some_and(|n| n == "package.json") {
            let depth = rel_path.components().count();
            if manifest_blob.as_ref().is_none_or(|(d, _)| depth < *d) {
                manifest_blob = Some((depth, content.clone()));
            }
        }

        entries.push(TarEntry {
            rel_path,
            content,
            mode,
            is_dir,
        });
    }

    let manifest_blob = manifest_blob
        .map(|(_, b)| b)
        .ok_or_else(|| anyhow!("package.json not found in tarball"))?;

    Ok((entries, manifest_blob))
}

/// Write collected tar entries into `dest`, preserving the archive's
/// directory layout (including the typical `package/` wrapper) so the
/// install phase can locate the real package root via `find_real_src`.
///
/// Entry modes were already passed through [`normalize_entry_mode`] at scan
/// time, so the executable bit npm packages rely on for `bin/` survives.
pub(crate) fn write_entries(entries: &[TarEntry], dest: &Path) -> Result<()> {
    for entry in entries {
        let full_path = dest.join(&entry.rel_path);
        if entry.is_dir {
            std::fs::create_dir_all(&full_path)
                .with_context(|| format!("failed to create directory {}", full_path.display()))?;
            continue;
        }
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        std::fs::write(&full_path, &entry.content)
            .with_context(|| format!("failed to write {}", full_path.display()))?;

        #[cfg(unix)]
        if entry.mode != 0o644
            && let Err(e) =
                std::fs::set_permissions(&full_path, std::fs::Permissions::from_mode(entry.mode))
        {
            tracing::warn!(
                "Failed to set mode {:o} on {}: {e}",
                entry.mode,
                full_path.display()
            );
        }
    }
    Ok(())
}

/// Returns `true` when a tar entry path is safe to join under an extraction
/// root: rejects absolute paths and any `..` component (Tar Slip).
///
/// Shared with pm's install-phase extractor so every extraction path in the
/// `~/.cache/nm/` tree applies the same traversal guard.
pub fn is_safe_tar_entry_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Unified tar-entry mode rule for every cache slot (registry, git, http,
/// file): npm-style normalization — every file becomes `0o644`, plus
/// `0o755` when any exec bit is set in the archive.
///
/// Real-world tarballs ship owner-only modes (the e2e suite pins
/// google-protobuf, whose entries are `0o640`); replaying them produces
/// node_modules that other users/processes cannot read. Normalizing keeps
/// installs world-readable, preserves the exec bit `bin/` scripts depend
/// on, inherently strips setuid/setgid/sticky so a crafted tarball cannot
/// plant a setuid binary in the shared cache, and guarantees the same
/// input bytes produce the same cache tree no matter which resolver
/// populated the slot.
pub fn normalize_entry_mode(raw_mode: u32) -> u32 {
    if raw_mode & 0o111 != 0 { 0o755 } else { 0o644 }
}

/// Estimate uncompressed size from the gzip ISIZE footer (last 4 bytes:
/// original size mod 2^32), bounded by sanity limits to deflect
/// crafted-footer allocation bombs. Falls back to `len * 10` when the
/// footer is implausible (below 16 bytes or above
/// [`MAX_UNCOMPRESSED_BYTES`]).
pub fn estimate_uncompressed_size(gzip_bytes: &[u8]) -> usize {
    const MIN: usize = 16;
    if gzip_bytes.len() < 4 {
        return gzip_bytes.len() * 10;
    }
    let footer = &gzip_bytes[gzip_bytes.len() - 4..];
    let size = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
    if (MIN..=(MAX_UNCOMPRESSED_BYTES as usize)).contains(&size) {
        size
    } else {
        gzip_bytes.len() * 10
    }
}

/// Shared tarball-commit pipeline used by the http and file tarball
/// resolvers.
///
/// Decompresses `gzip_bytes`, parses the embedded `package.json`, stamps
/// `pinned_url` onto `dist.tarball` via `finalize_non_registry_manifest`,
/// and atomically commits the extracted tree to
/// `<cache_dir>/<name>/<slot>/package/`.
///
/// Cache is idempotent: the `_resolved` marker short-circuits re-extraction
/// on warm runs. Returns the finalized manifest so BFS can produce a
/// [`crate::traits::registry::ResolvedPackage`].
pub(crate) fn commit_tarball_bytes(
    cache_dir: &Path,
    gzip_bytes: &[u8],
    pinned_url: String,
    slot: &str,
) -> Result<CoreVersionManifest> {
    let decompressed = gzip_decompress(gzip_bytes)?;
    let (entries, manifest_blob) = scan_tarball(&decompressed)?;

    let mut manifest: CoreVersionManifest = serde_json::from_slice(&manifest_blob)
        .context("failed to parse package.json from tarball")?;
    finalize_non_registry_manifest(&mut manifest, pinned_url)?;

    let package_dir = cache_dir.join(&manifest.name).join(slot);
    if package_dir.join("_resolved").exists() {
        return Ok(manifest);
    }
    commit_cache_dir_atomic(&package_dir, |stage| write_entries(&entries, stage))?;

    Ok(manifest)
}

/// Decompress a gzip stream with libdeflate, sizing the output buffer from
/// [`estimate_uncompressed_size`] and retrying once with a 4x buffer when
/// the estimate proves too small.
pub fn gzip_decompress(gzip_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decompressor = libdeflater::Decompressor::new();
    let estimated = estimate_uncompressed_size(gzip_bytes);
    let mut output = vec![0u8; estimated];
    let actual = match decompressor.gzip_decompress(gzip_bytes, &mut output) {
        Ok(n) => n,
        Err(libdeflater::DecompressionError::InsufficientSpace) => {
            output.resize(estimated.saturating_mul(4).max(1024), 0);
            decompressor
                .gzip_decompress(gzip_bytes, &mut output)
                .context("gzip decompression failed (even with 4x buffer)")?
        }
        Err(e) => return Err(anyhow!("gzip decompression failed: {e:?}")),
    };
    output.truncate(actual);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_path_predicate_rejects_traversal() {
        assert!(is_safe_tar_entry_path(Path::new("package/index.js")));
        assert!(is_safe_tar_entry_path(Path::new("package.json")));
        assert!(!is_safe_tar_entry_path(Path::new("/etc/passwd")));
        assert!(!is_safe_tar_entry_path(Path::new("package/../../evil")));
        assert!(!is_safe_tar_entry_path(Path::new("../outside")));
    }

    #[test]
    fn mode_rule_normalizes_to_world_readable() {
        assert_eq!(normalize_entry_mode(0o644), 0o644);
        assert_eq!(normalize_entry_mode(0o755), 0o755);
        // Owner-only modes widen to world-readable — google-protobuf ships
        // 0o640 entries and installs must stay readable (e2e-pinned).
        assert_eq!(normalize_entry_mode(0o600), 0o644);
        assert_eq!(normalize_entry_mode(0o640), 0o644);
        // Any exec bit ⇒ 0o755; setuid/setgid never survive.
        assert_eq!(normalize_entry_mode(0o700), 0o755);
        assert_eq!(normalize_entry_mode(0o4755), 0o755);
        assert_eq!(normalize_entry_mode(0o2644), 0o644);
    }

    #[test]
    fn isize_footer_estimate_is_bounded() {
        // Plausible footer → trusted verbatim.
        let mut bytes = vec![0u8; 100];
        bytes[96..].copy_from_slice(&1024u32.to_le_bytes());
        assert_eq!(estimate_uncompressed_size(&bytes), 1024);
        // Implausible footer (zero) → fallback to len * 10.
        let zeros = vec![0u8; 100];
        assert_eq!(estimate_uncompressed_size(&zeros), 1000);
        // Tiny input → fallback.
        assert_eq!(estimate_uncompressed_size(&[1, 2]), 20);
    }
}
