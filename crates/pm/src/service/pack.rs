use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use sha1::Digest;
use std::path::{Path, PathBuf};
use tar::Builder;

use crate::util::packfile::collect_pack_files;

pub struct PackedFile {
    pub path: String,
    pub size: u64,
}

pub struct PackResult {
    pub tarball_path: Option<PathBuf>,
    pub files: Vec<PackedFile>,
    pub name: String,
    pub version: String,
    pub shasum: String,
    pub integrity: String,
    pub unpacked_size: u64,
    pub packed_size: u64,
    pub file_count: usize,
}

pub async fn pack(package_root: &Path, dry_run: bool) -> Result<PackResult> {
    let package_json_path = package_root.join("package.json");
    let data: serde_json::Value = serde_json::from_str(
        &crate::fs::read_to_string(&package_json_path)
            .await
            .context("Failed to read package.json")?,
    )
    .context("Failed to parse package.json")?;

    let name = data["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'name' field in package.json"))?
        .to_string();
    let version = data["version"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing 'version' field in package.json"))?
        .to_string();

    // Collect files
    let pack_config = collect_pack_files(package_root).await?;

    let packed_files: Vec<PackedFile> = pack_config
        .files
        .iter()
        .map(|f| {
            let size = std::fs::metadata(package_root.join(f))
                .map(|m| m.len())
                .unwrap_or(0);
            PackedFile {
                path: f.to_string_lossy().to_string(),
                size,
            }
        })
        .collect();

    let file_count = packed_files.len();
    let unpacked_size = pack_config.total_size;

    if dry_run {
        return Ok(PackResult {
            tarball_path: None,
            files: packed_files,
            name,
            version,
            shasum: String::new(),
            integrity: String::new(),
            unpacked_size,
            packed_size: 0,
            file_count,
        });
    }

    // Create tarball in memory
    let tar_data = {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = Builder::new(&mut encoder);

            for file_path in &pack_config.files {
                let full_path = package_root.join(file_path);
                let archive_path = Path::new("package").join(file_path);
                builder
                    .append_path_with_name(&full_path, &archive_path)
                    .with_context(|| {
                        format!("Failed to add {} to tarball", file_path.display())
                    })?;
            }

            builder.finish()?;
        }
        encoder.finish()?
    };

    // Calculate SHA-1 (shasum)
    let shasum = {
        let hash = sha1::Sha1::digest(&tar_data);
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };

    // Calculate SHA-512 (integrity, SRI format)
    let integrity = {
        let hash = sha2::Sha512::digest(&tar_data);
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(hash);
        format!("sha512-{b64}")
    };

    let packed_size = tar_data.len() as u64;

    // Write tarball to disk
    let tarball_name = format!(
        "{}-{}.tgz",
        name.replace('/', "-").replace('@', ""),
        version
    );
    let tarball_path = package_root.join(&tarball_name);

    std::fs::write(&tarball_path, &tar_data)
        .with_context(|| format!("Failed to write tarball to {}", tarball_path.display()))?;

    Ok(PackResult {
        tarball_path: Some(tarball_path),
        files: packed_files,
        name,
        version,
        shasum,
        integrity,
        unpacked_size,
        packed_size,
        file_count,
    })
}
