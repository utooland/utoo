use std::{io, path::Path};
use tokio_fs_ext::{offload, watch, Metadata, ReadDir};

pub struct OpfsOffload;

impl offload::FsOffload for OpfsOffload {
    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        opfs_project::read(path).await
    }

    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        opfs_project::write(path, content).await
    }

    async fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        opfs_project::copy(from, to).await
    }

    async fn read_dir(&self, path: impl AsRef<Path>) -> io::Result<ReadDir> {
        opfs_project::read_dir(path).await.map(ReadDir::from_iter)
    }

    async fn create_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        opfs_project::create_dir(path).await
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        opfs_project::create_dir_all(path).await
    }

    async fn remove_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        opfs_project::remove_file(path).await
    }

    async fn remove_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        opfs_project::remove_dir(path).await
    }

    async fn remove_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        opfs_project::remove_dir_all(path).await
    }

    async fn metadata(&self, path: impl AsRef<Path>) -> io::Result<Metadata> {
        opfs_project::metadata(path).await
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
