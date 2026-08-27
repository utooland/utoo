use std::{
    fs::{File, OpenOptions},
    io::Write,
    mem::ManuallyDrop,
    sync::Mutex,
};

use anyhow::Context;
use napi::{
    Env,
    bindgen_prelude::{External, ExternalRef, PromiseRaw},
};
use napi_derive::napi;

type JsLockfile = Mutex<ManuallyDrop<Option<LockfileInner>>>;

pub struct LockfileInner {
    file: File,
    #[cfg(not(windows))]
    path: std::path::PathBuf,
}

#[napi(ts_return_type = "{ __napiType: \"Lockfile\" } | null")]
pub fn lockfile_try_acquire_sync(
    path: String,
    content: Option<String>,
) -> napi::Result<Option<External<JsLockfile>>> {
    #[cfg(windows)]
    return {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::{Foundation, Storage::FileSystem};

        let mut open_options = OpenOptions::new();
        open_options.write(true).create(true).truncate(true);
        open_options
            .share_mode(FileSystem::FILE_SHARE_READ | FileSystem::FILE_SHARE_DELETE)
            .custom_flags(FileSystem::FILE_FLAG_DELETE_ON_CLOSE);

        match open_options.open(&path) {
            Ok(mut file) => {
                if let Some(ref data) = content {
                    file.write_all(data.as_bytes())?;
                    file.flush()?;
                }
                Ok(Some(External::new(Mutex::new(ManuallyDrop::new(Some(
                    LockfileInner { file },
                ))))))
            }
            Err(err)
                if err.raw_os_error()
                    == Some(Foundation::ERROR_SHARING_VIOLATION.try_into().unwrap()) =>
            {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    };

    #[cfg(not(windows))]
    return {
        use std::{fs::TryLockError, io::Seek};

        let mut open_options = OpenOptions::new();
        open_options.write(true).create(true).read(true);

        let file = open_options.open(&path)?;
        match file.try_lock() {
            Ok(_) => {
                file.set_len(0)?;
                (&file).seek(std::io::SeekFrom::Start(0))?;
                if let Some(ref data) = content {
                    (&file).write_all(data.as_bytes())?;
                    (&file).flush()?;
                }
                Ok(Some(External::new(Mutex::new(ManuallyDrop::new(Some(
                    LockfileInner {
                        file,
                        path: path.into(),
                    },
                ))))))
            }
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err.into()),
        }
    };
}

#[napi(ts_return_type = "Promise<{ __napiType: \"Lockfile\" } | null>")]
pub async fn lockfile_try_acquire(
    path: String,
    content: Option<String>,
) -> napi::Result<Option<External<JsLockfile>>> {
    tokio::task::spawn_blocking(move || lockfile_try_acquire_sync(path, content))
        .await
        .context("panicked while attempting to acquire lockfile")?
}

#[napi]
pub fn lockfile_unlock_sync(
    #[napi(ts_arg_type = "{ __napiType: \"Lockfile\" }")] lockfile: ExternalRef<JsLockfile>,
) {
    let Some(inner) = take_lockfile_inner(&lockfile) else {
        return;
    };

    unlock_inner(inner);
}

#[napi]
pub fn lockfile_unlock<'env>(
    env: &'env Env,
    #[napi(ts_arg_type = "{ __napiType: \"Lockfile\" }")] lockfile: ExternalRef<JsLockfile>,
) -> napi::Result<PromiseRaw<'env, ()>> {
    // Take the owned inner out on the JS thread (the `ExternalRef` is `!Send`), then release the
    // lock on the blocking pool so the unlink and close don't stall the Node.js event loop.
    let inner = take_lockfile_inner(&lockfile);
    env.spawn_future(async move {
        let Some(inner) = inner else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || unlock_inner(inner))
            .await
            .context("panicked while attempting to unlock lockfile")?;
        Ok(())
    })
}

fn take_lockfile_inner(lockfile: &JsLockfile) -> Option<LockfileInner> {
    lockfile
        .lock()
        .expect("poisoned: another thread panicked while unlocking this lockfile")
        .take()
}

fn unlock_inner(inner: LockfileInner) {
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(inner.path);

    drop(inner.file);
}
