use std::{io, path::Path};
use tokio_fs_ext::{offload, watch, Metadata, ReadDir};

use crate::pm::{with_project, OPFS_PROJECT};

pub struct OpfsOffload;

impl offload::FsOffload for OpfsOffload {
    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("OpfsProject not initialised"))?;
        project.read(path).await.map(|b| b.to_vec())
    }

    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        tokio_fs_ext::write(path, content).await
    }

    async fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        tokio_fs_ext::copy(from, to).await
    }

    async fn read_dir(&self, path: impl AsRef<Path>) -> io::Result<ReadDir> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("OpfsProject not initialised"))?;
        project.read_dir(path).await.map(ReadDir::from_iter)
    }

    async fn create_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        tokio_fs_ext::create_dir(path).await
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        tokio_fs_ext::create_dir_all(path).await
    }

    async fn remove_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        tokio_fs_ext::remove_file(path).await
    }

    async fn remove_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        tokio_fs_ext::remove_dir(path).await
    }

    async fn remove_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        tokio_fs_ext::remove_dir_all(path).await
    }

    async fn metadata(&self, path: impl AsRef<Path>) -> io::Result<Metadata> {
        let project = OPFS_PROJECT
            .read()
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("OpfsProject not initialised"))?;
        project.metadata(path).await
    }

    async fn watch_dir(
        &self,
        path: impl AsRef<Path>,
        recursive: bool,
        cb: impl Fn(watch::event::Event) + Send + Sync + 'static,
    ) -> io::Result<()> {
        watch::watch_dir(path, recursive, cb).await
    }
}
