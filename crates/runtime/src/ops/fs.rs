use deno_core::op2;
use deno_core::JsBuffer;
use deno_error::JsErrorBox;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Serde types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct StatResult {
    pub size: u64,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub mode: u32,
    pub mtime_ms: f64,
    pub atime_ms: f64,
    pub ctime_ms: f64,
    pub birthtime_ms: f64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
    pub is_directory: bool,
    pub is_symlink: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn metadata_to_stat(meta: &std::fs::Metadata) -> StatResult {
    use std::os::unix::fs::MetadataExt;
    use std::time::UNIX_EPOCH;

    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    let atime_ms = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    let birthtime_ms = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    // On Unix, ctime is status change time (not creation). Use mtime as approximation.
    let ctime_ms = mtime_ms;

    StatResult {
        size: meta.len(),
        is_file: meta.is_file(),
        is_directory: meta.is_dir(),
        is_symlink: meta.is_symlink(),
        mode: meta.mode(),
        mtime_ms,
        atime_ms,
        ctime_ms,
        birthtime_ms,
        dev: meta.dev(),
        ino: meta.ino(),
        nlink: meta.nlink(),
    }
}

fn map_io_err(e: std::io::Error) -> JsErrorBox {
    JsErrorBox::generic(e.to_string())
}

// ---------------------------------------------------------------------------
// File read / write — async
// ---------------------------------------------------------------------------

#[op2]
#[buffer]
pub async fn op_fs_read_file(#[string] path: String) -> Result<Vec<u8>, JsErrorBox> {
    tokio::fs::read(&path).await.map_err(map_io_err)
}

#[op2]
#[string]
pub async fn op_fs_read_text_file(#[string] path: String) -> Result<String, JsErrorBox> {
    tokio::fs::read_to_string(&path).await.map_err(map_io_err)
}

#[op2]
pub async fn op_fs_write_file(
    #[string] path: String,
    #[buffer] data: JsBuffer,
) -> Result<(), JsErrorBox> {
    tokio::fs::write(&path, data.as_ref()).await.map_err(map_io_err)
}

#[op2]
pub async fn op_fs_append_file(
    #[string] path: String,
    #[buffer] data: JsBuffer,
) -> Result<(), JsErrorBox> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .await
        .map_err(map_io_err)?;
    file.write_all(data.as_ref()).await.map_err(map_io_err)
}

// ---------------------------------------------------------------------------
// File read / write — sync
// ---------------------------------------------------------------------------

#[op2]
#[buffer]
pub fn op_fs_read_file_sync(#[string] path: &str) -> Result<Vec<u8>, JsErrorBox> {
    std::fs::read(path).map_err(map_io_err)
}

#[op2]
#[string]
pub fn op_fs_read_text_file_sync(#[string] path: &str) -> Result<String, JsErrorBox> {
    std::fs::read_to_string(path).map_err(map_io_err)
}

#[op2(fast)]
pub fn op_fs_write_file_sync(
    #[string] path: &str,
    #[buffer] data: &[u8],
) -> Result<(), JsErrorBox> {
    std::fs::write(path, data).map_err(map_io_err)
}

#[op2(fast)]
pub fn op_fs_append_file_sync(
    #[string] path: &str,
    #[buffer] data: &[u8],
) -> Result<(), JsErrorBox> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(map_io_err)?;
    file.write_all(data).map_err(map_io_err)
}

// ---------------------------------------------------------------------------
// Directory ops — async
// ---------------------------------------------------------------------------

#[op2]
#[serde]
pub async fn op_fs_readdir(#[string] path: String) -> Result<Vec<DirEntry>, JsErrorBox> {
    let mut entries = Vec::new();
    let mut rd = tokio::fs::read_dir(&path).await.map_err(map_io_err)?;
    while let Some(entry) = rd.next_entry().await.map_err(map_io_err)? {
        let ft = entry.file_type().await.map_err(map_io_err)?;
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: ft.is_file(),
            is_directory: ft.is_dir(),
            is_symlink: ft.is_symlink(),
        });
    }
    Ok(entries)
}

#[op2]
pub async fn op_fs_mkdir(
    #[string] path: String,
    recursive: bool,
) -> Result<(), JsErrorBox> {
    if recursive {
        tokio::fs::create_dir_all(&path).await.map_err(map_io_err)
    } else {
        tokio::fs::create_dir(&path).await.map_err(map_io_err)
    }
}

// ---------------------------------------------------------------------------
// Directory ops — sync
// ---------------------------------------------------------------------------

#[op2]
#[serde]
pub fn op_fs_readdir_sync(#[string] path: &str) -> Result<Vec<DirEntry>, JsErrorBox> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(map_io_err)? {
        let entry = entry.map_err(map_io_err)?;
        let ft = entry.file_type().map_err(map_io_err)?;
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: ft.is_file(),
            is_directory: ft.is_dir(),
            is_symlink: ft.is_symlink(),
        });
    }
    Ok(entries)
}

#[op2(fast)]
pub fn op_fs_mkdir_sync(
    #[string] path: &str,
    recursive: bool,
) -> Result<(), JsErrorBox> {
    if recursive {
        std::fs::create_dir_all(path).map_err(map_io_err)
    } else {
        std::fs::create_dir(path).map_err(map_io_err)
    }
}

// ---------------------------------------------------------------------------
// Stat — async & sync
// ---------------------------------------------------------------------------

#[op2]
#[serde]
pub async fn op_fs_stat(#[string] path: String) -> Result<StatResult, JsErrorBox> {
    let meta = tokio::fs::metadata(&path).await.map_err(map_io_err)?;
    Ok(metadata_to_stat(&meta))
}

#[op2]
#[serde]
pub fn op_fs_stat_sync(#[string] path: &str) -> Result<StatResult, JsErrorBox> {
    let meta = std::fs::metadata(path).map_err(map_io_err)?;
    Ok(metadata_to_stat(&meta))
}

#[op2]
#[serde]
pub async fn op_fs_lstat(#[string] path: String) -> Result<StatResult, JsErrorBox> {
    let meta = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(map_io_err)?;
    Ok(metadata_to_stat(&meta))
}

#[op2]
#[serde]
pub fn op_fs_lstat_sync(#[string] path: &str) -> Result<StatResult, JsErrorBox> {
    let meta = std::fs::symlink_metadata(path).map_err(map_io_err)?;
    Ok(metadata_to_stat(&meta))
}

// ---------------------------------------------------------------------------
// File management — async
// ---------------------------------------------------------------------------

#[op2]
pub async fn op_fs_unlink(#[string] path: String) -> Result<(), JsErrorBox> {
    tokio::fs::remove_file(&path).await.map_err(map_io_err)
}

#[op2]
pub async fn op_fs_rename(
    #[string] from: String,
    #[string] to: String,
) -> Result<(), JsErrorBox> {
    tokio::fs::rename(&from, &to).await.map_err(map_io_err)
}

#[op2]
pub async fn op_fs_copy_file(
    #[string] from: String,
    #[string] to: String,
) -> Result<(), JsErrorBox> {
    tokio::fs::copy(&from, &to).await.map_err(map_io_err)?;
    Ok(())
}

#[op2]
pub async fn op_fs_rm(
    #[string] path: String,
    recursive: bool,
) -> Result<(), JsErrorBox> {
    if recursive {
        tokio::fs::remove_dir_all(&path).await.map_err(map_io_err)
    } else {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(_) => tokio::fs::remove_dir(&path).await.map_err(map_io_err),
        }
    }
}

#[op2]
pub async fn op_fs_access(
    #[string] path: String,
    _mode: u32,
) -> Result<(), JsErrorBox> {
    tokio::fs::metadata(&path).await.map_err(map_io_err)?;
    Ok(())
}

#[op2]
pub async fn op_fs_chmod(
    #[string] path: String,
    mode: u32,
) -> Result<(), JsErrorBox> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    tokio::fs::set_permissions(&path, perms)
        .await
        .map_err(map_io_err)
}

#[op2]
#[string]
pub async fn op_fs_realpath(#[string] path: String) -> Result<String, JsErrorBox> {
    let p = tokio::fs::canonicalize(&path).await.map_err(map_io_err)?;
    Ok(p.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// File management — sync
// ---------------------------------------------------------------------------

#[op2(fast)]
pub fn op_fs_unlink_sync(#[string] path: &str) -> Result<(), JsErrorBox> {
    std::fs::remove_file(path).map_err(map_io_err)
}

#[op2(fast)]
pub fn op_fs_rename_sync(
    #[string] from: &str,
    #[string] to: &str,
) -> Result<(), JsErrorBox> {
    std::fs::rename(from, to).map_err(map_io_err)
}

#[op2(fast)]
pub fn op_fs_copy_file_sync(
    #[string] from: &str,
    #[string] to: &str,
) -> Result<(), JsErrorBox> {
    std::fs::copy(from, to).map_err(map_io_err)?;
    Ok(())
}

#[op2(fast)]
pub fn op_fs_rm_sync(
    #[string] path: &str,
    recursive: bool,
) -> Result<(), JsErrorBox> {
    if recursive {
        std::fs::remove_dir_all(path).map_err(map_io_err)
    } else {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(_) => std::fs::remove_dir(path).map_err(map_io_err),
        }
    }
}

#[op2(fast)]
pub fn op_fs_exists_sync(#[string] path: &str) -> bool {
    std::path::Path::new(path).exists()
}

#[op2(fast)]
pub fn op_fs_access_sync(
    #[string] path: &str,
    _mode: u32,
) -> Result<(), JsErrorBox> {
    std::fs::metadata(path).map_err(map_io_err)?;
    Ok(())
}

#[op2(fast)]
pub fn op_fs_chmod_sync(
    #[string] path: &str,
    mode: u32,
) -> Result<(), JsErrorBox> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms).map_err(map_io_err)
}

#[op2]
#[string]
pub fn op_fs_realpath_sync(#[string] path: &str) -> Result<String, JsErrorBox> {
    let p = std::fs::canonicalize(path).map_err(map_io_err)?;
    Ok(p.to_string_lossy().into_owned())
}
