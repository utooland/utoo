use tokio_fs_ext::offload;

pub struct OpfsOffload;

impl offload::FsOffload for OpfsOffload {
    async fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        todo!()
    }

    async fn write(&self, path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> io::Result<()> {
        todo!()
    }

    async fn copy(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
        todo!()
    }

    async fn read_dir(&self, path: impl AsRef<Path>) -> io::Result<ReadDir> {
        todo!()
    }

    async fn create_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn remove_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn remove_dir(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn remove_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        todo!()
    }

    async fn metadata(&self, path: impl AsRef<Path>) -> io::Result<Metadata> {
        todo!()
    }
}
