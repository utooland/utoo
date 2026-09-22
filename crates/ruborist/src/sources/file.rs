//! File source I/O. The resolver alone creates graph nodes and edges.
use crate::model::package_json::PackageJson;
use crate::traits::registry::ResolvedPackage;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) enum FileSource {
    Directory {
        path: PathBuf,
        package: Box<PackageJson>,
    },
    Tarball(ResolvedPackage),
}

pub(crate) enum FileSourceError {
    Access(anyhow::Error),
    DirectoryManifest(anyhow::Error),
    Tarball(anyhow::Error),
}

pub(crate) async fn read_file_source(path: PathBuf) -> Result<FileSource, FileSourceError> {
    let metadata = std::fs::metadata(&path).map_err(|error| {
        FileSourceError::Access(
            anyhow::Error::new(error).context(format!("file: target {}", path.display())),
        )
    })?;
    if metadata.is_dir() {
        let package = crate::model::util::read_package_json(&path)
            .await
            .map_err(FileSourceError::DirectoryManifest)?;
        return Ok(FileSource::Directory {
            path,
            package: Box::new(package),
        });
    }
    let manifest = super::tar::read_local_tarball_manifest(path)
        .await
        .map_err(FileSourceError::Tarball)?;
    Ok(FileSource::Tarball(ResolvedPackage::from_manifest(
        manifest.name.clone(),
        Arc::new(manifest),
    )))
}
