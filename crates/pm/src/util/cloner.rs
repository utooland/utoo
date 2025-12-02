use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::retry::create_retry_strategy;
use tokio::fs;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use tokio_retry::Retry;

#[cfg(target_os = "macos")]
use libc::clonefile;
#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "linux")]
mod linux_clone {
    use anyhow::{Context, Result};
    use std::os::unix::io::AsRawFd;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::fs;

    // Cache for copy_file_range support
    static COPY_FILE_RANGE_SUPPORTED: AtomicBool = AtomicBool::new(true);

    // Cache for FICLONE support
    static FICLONE_SUPPORTED: AtomicBool = AtomicBool::new(true);

    // Check if FICLONE is supported
    fn is_ficlone_supported() -> bool {
        FICLONE_SUPPORTED.load(Ordering::Relaxed)
    }

    // Check if copy_file_range is supported
    fn is_copy_file_range_supported() -> bool {
        COPY_FILE_RANGE_SUPPORTED.load(Ordering::Relaxed)
    }

    // Copy a single file using FICLONE
    async fn copy_file_with_ficlone(src: &Path, dst: &Path) -> Result<()> {
        let src_file = tokio::fs::File::open(src).await?;
        let src_metadata = src_file.metadata().await?;
        let src_permissions = src_metadata.permissions();

        // Create destination file
        let dst_file = tokio::fs::File::create(dst).await?;

        let src_fd = src_file.as_raw_fd();
        let dst_fd = dst_file.as_raw_fd();

        // Use spawn_blocking to avoid blocking the async runtime
        let result = tokio::task::spawn_blocking(move || {
            let ret = unsafe { libc::ioctl(dst_fd, libc::FICLONE, src_fd) };

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                Err(err)
            } else {
                Ok(())
            }
        })
        .await?;

        if let Err(e) = result {
            FICLONE_SUPPORTED.store(false, Ordering::Relaxed);
            return Err(anyhow::anyhow!("FICLONE not supported: {}", e));
        }

        // Preserve permissions after successful copy
        fs::set_permissions(dst, src_permissions).await?;

        Ok(())
    }

    // Copy a single file using copy_file_range
    async fn copy_file_with_range(src: &Path, dst: &Path) -> Result<()> {
        let src_file = tokio::fs::File::open(src).await?;
        let metadata = src_file.metadata().await?;
        let file_size = metadata.len() as usize;
        let src_permissions = metadata.permissions();

        // Create destination file
        let dst_file = tokio::fs::File::create(dst).await?;

        let src_fd = src_file.as_raw_fd();
        let dst_fd = dst_file.as_raw_fd();

        // Use spawn_blocking to avoid blocking the async runtime
        let result = tokio::task::spawn_blocking(move || {
            let mut offset: i64 = 0;
            let ret = unsafe {
                libc::copy_file_range(src_fd, &mut offset, dst_fd, &mut offset, file_size, 0)
            };

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                Err(err)
            } else {
                Ok(())
            }
        })
        .await?;

        if let Err(e) = result {
            COPY_FILE_RANGE_SUPPORTED.store(false, Ordering::Relaxed);
            return Err(anyhow::anyhow!("copy_file_range not supported: {}", e));
        }

        // Preserve permissions after successful copy
        fs::set_permissions(dst, src_permissions).await?;

        Ok(())
    }

    // Fast copy a single file using the best available method
    async fn fast_copy_file(src: &Path, dst: &Path) -> Result<()> {
        // Try FICLONE if supported
        if is_ficlone_supported() {
            match copy_file_with_ficlone(src, dst).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::debug!("FICLONE failed: {e}, trying copy_file_range");
                }
            }
        }

        // Try copy_file_range if supported
        if is_copy_file_range_supported() {
            match copy_file_with_range(src, dst).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    tracing::debug!("copy_file_range failed: {e}, using regular copy");
                }
            }
        }

        // Fallback to regular copy
        tokio::fs::copy(src, dst).await.with_context(|| {
            format!(
                "Failed to copy file from {} to {}",
                src.display(),
                dst.display()
            )
        })?;

        // Preserve permissions for regular copy fallback
        let src_metadata = fs::metadata(src).await?;
        fs::set_permissions(dst, src_metadata.permissions()).await?;

        Ok(())
    }

    // Fast copy using the best available method
    async fn fast_copy(src: &Path, dst: &Path) -> Result<()> {
        // Create destination directory
        fs::create_dir_all(dst).await?;

        // Copy all files in the directory
        let mut read_dir = fs::read_dir(src).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry_path.file_name().unwrap();
            let target_path = dst.join(file_name);

            if entry.metadata().await?.is_dir() {
                Box::pin(fast_copy(&entry_path, &target_path)).await?;
            } else {
                fast_copy_file(&entry_path, &target_path).await?;
            }
        }
        Ok(())
    }

    // Check if the package has install scripts
    async fn has_install_script(src: &Path) -> bool {
        if let Some(parent) = src.parent() {
            let flag_path = parent.join("_hasInstallScript");
            if let Ok(metadata) = fs::metadata(&flag_path).await {
                return metadata.is_file();
            }
        }
        false
    }

    pub async fn clone_dir(src: &Path, dst: &Path) -> Result<()> {
        if !fs::metadata(src).await?.is_dir() {
            return Err(anyhow::anyhow!("Source is not a directory"));
        }

        // Create destination directory
        fs::create_dir_all(dst).await?;

        // Check if the package has install scripts
        if has_install_script(src).await {
            tracing::debug!(
                "Package has install scripts, using fast copy for directory {} to {}",
                src.display(),
                dst.display()
            );
            return fast_copy(src, dst).await;
        }

        // For directories without install scripts, recursively create the directory structure
        let mut read_dir = fs::read_dir(src).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry_path.file_name().unwrap();
            let target_path = dst.join(file_name);

            if entry.metadata().await?.is_dir() {
                Box::pin(clone_dir(&entry_path, &target_path)).await?;
            } else {
                // Try hardlink first for files in packages without install scripts
                if let Err(e) = fs::hard_link(&entry_path, &target_path).await {
                    tracing::debug!(
                        "Failed to create hardlink for file from {} to {}: {}, trying fast copy",
                        entry_path.display(),
                        target_path.display(),
                        e
                    );
                    // If hardlink fails, fallback to fast copy
                    fast_copy_file(&entry_path, &target_path).await?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows_clone {
    use anyhow::{Context, Result};
    use std::path::Path;
    use tokio::fs;

    // Copy a single file using regular copy
    async fn fast_copy_file(src: &Path, dst: &Path) -> Result<()> {
        tokio::fs::copy(src, dst).await.with_context(|| {
            format!(
                "Failed to copy file from {} to {}",
                src.display(),
                dst.display()
            )
        })?;
        Ok(())
    }

    // Fast copy directory using regular copy for packages with install scripts
    async fn fast_copy(src: &Path, dst: &Path) -> Result<()> {
        // Create destination directory
        fs::create_dir_all(dst).await?;

        // Copy all files in the directory
        let mut read_dir = fs::read_dir(src).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry_path.file_name().unwrap();
            let target_path = dst.join(file_name);

            if entry.metadata().await?.is_dir() {
                Box::pin(fast_copy(&entry_path, &target_path)).await?;
            } else {
                fast_copy_file(&entry_path, &target_path).await?;
            }
        }
        Ok(())
    }

    // Check if the package has install scripts
    async fn has_install_script(src: &Path) -> bool {
        if let Some(parent) = src.parent() {
            let flag_path = parent.join("_hasInstallScript");
            if let Ok(metadata) = fs::metadata(&flag_path).await {
                return metadata.is_file();
            }
        }
        false
    }

    pub async fn clone_dir(src: &Path, dst: &Path) -> Result<()> {
        if !fs::metadata(src).await?.is_dir() {
            return Err(anyhow::anyhow!("Source is not a directory"));
        }

        // Create destination directory
        fs::create_dir_all(dst).await?;

        // Check if the package has install scripts
        if has_install_script(src).await {
            tracing::debug!(
                "Package has install scripts, using fast copy for directory {} to {}",
                src.display(),
                dst.display()
            );
            return fast_copy(src, dst).await;
        }

        // For directories without install scripts, recursively create the directory structure
        // and use hardlinks where possible
        let mut read_dir = fs::read_dir(src).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry_path.file_name().unwrap();
            let target_path = dst.join(file_name);

            if entry.metadata().await?.is_dir() {
                Box::pin(clone_dir(&entry_path, &target_path)).await?;
            } else {
                // Try hardlink first for files in packages without install scripts
                if let Err(e) = fs::hard_link(&entry_path, &target_path).await {
                    tracing::debug!(
                        "Failed to create hardlink for file from {} to {}: {}, using regular copy",
                        entry_path.display(),
                        target_path.display(),
                        e
                    );
                    // If hardlink fails, fallback to regular copy
                    tokio::fs::copy(&entry_path, &target_path)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to copy file from {} to {}",
                                entry_path.display(),
                                target_path.display()
                            )
                        })?;
                }
            }
        }

        Ok(())
    }
}

pub async fn validate_directory(src: &Path, dst: &Path) -> Result<bool> {
    if !tokio::fs::try_exists(dst).await? {
        return Ok(false);
    }

    if !fs::metadata(src).await?.is_dir() || !fs::metadata(dst).await?.is_dir() {
        tracing::debug!("validating failed, since it's not a directory");
        return Ok(false);
    }

    #[derive(Debug)]
    struct EntryInfo {
        path: PathBuf,
        is_dir: bool,
        size: u64,
    }

    async fn collect_entries(dir: &Path, ignore: Option<&[&str]>) -> Result<Vec<EntryInfo>> {
        let mut entries = Vec::new();
        let mut read_dir = fs::read_dir(dir)
            .await
            .with_context(|| format!("Failed to read directory {}", dir.display()))?;

        while let Some(entry) = read_dir.next_entry().await? {
            if let Some(ignore_list) = ignore
                && let Some(file_name) = entry.path().file_name()
                && ignore_list.contains(&file_name.to_str().unwrap_or_default())
            {
                continue;
            }

            let metadata = entry.metadata().await.with_context(|| {
                format!("Failed to get metadata for {}", entry.path().display())
            })?;
            entries.push(EntryInfo {
                path: entry.path(),
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
            });
        }
        Ok(entries)
    }

    let mut src_entries = collect_entries(src, Some(&["node_modules"]))
        .await
        .with_context(|| format!("Failed to collect entries for {}", src.display()))?;
    let mut dst_entries = collect_entries(dst, Some(&["node_modules"]))
        .await
        .with_context(|| format!("Failed to collect entries for {}", dst.display()))?;

    src_entries.sort_by_key(|e| e.path.clone());
    dst_entries.sort_by_key(|e| e.path.clone());

    if src_entries.len() != dst_entries.len() {
        tracing::debug!(
            "validating failed {}:{} to {}:{}, since entries length is not equal\nsrc entries: {:?}\ndst entries: {:?}",
            src.display(),
            src_entries.len(),
            dst.display(),
            dst_entries.len(),
            src_entries
                .iter()
                .map(|e| e.path.file_name().unwrap_or_default())
                .collect::<Vec<_>>(),
            dst_entries
                .iter()
                .map(|e| e.path.file_name().unwrap_or_default())
                .collect::<Vec<_>>()
        );
        return Ok(false);
    }

    for (src_entry, dst_entry) in src_entries.iter().zip(dst_entries.iter()) {
        if src_entry.is_dir && dst_entry.is_dir {
            let future = validate_directory(&src_entry.path, &dst_entry.path);
            if !Box::pin(future).await? {
                return Ok(false);
            }
        } else if !src_entry.is_dir && !dst_entry.is_dir {
            if src_entry.size != dst_entry.size {
                tracing::debug!(
                    "validating failed {}:{} to {}:{}, since diff size",
                    src_entry.path.display(),
                    src_entry.size,
                    dst_entry.path.display(),
                    dst_entry.size
                );
                return Ok(false);
            }
        } else {
            tracing::debug!(
                "validating failed {}:{} to {}:{}, since diff size",
                src_entry.path.display(),
                src_entry.size,
                dst_entry.path.display(),
                dst_entry.size
            );
            return Ok(false);
        }
    }

    Ok(true)
}

// find the first non built subdirectory
pub async fn find_real_src<P: AsRef<Path>>(src: P) -> Option<PathBuf> {
    let mut read_dir = fs::read_dir(src.as_ref()).await.ok()?;
    while let Some(entry) = read_dir.next_entry().await.ok()? {
        if let Ok(metadata) = entry.metadata().await
            && metadata.is_dir()
            && let Some(name) = entry.path().file_name()
            && name.to_str().unwrap_or_default() != ".utoo_built"
        {
            return Some(entry.path());
        }
    }
    None
}

pub async fn clone(src: &Path, dst: &Path, find_real: bool) -> Result<()> {
    let real_src = if find_real {
        find_real_src(src)
            .await
            .ok_or_else(|| anyhow::anyhow!("Cannot find valid source directory in {src:?}"))?
    } else {
        src.to_path_buf()
    };

    if tokio::fs::try_exists(dst).await? {
        let is_valid = validate_directory(&real_src, dst)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("validate_directory error: {e}, will override target directory");
                false
            });

        if is_valid {
            tracing::debug!(
                "Target directory {} already exists and validation passed, skipping clone",
                dst.display()
            );
            return Ok(());
        }

        tracing::debug!("{real_src:?} --> {dst:?} overrides");
        if let Err(e) = fs::remove_dir_all(dst).await {
            tracing::warn!("Failed to clean target directory {}: {}", dst.display(), e);
        }
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).await?;
    }

    #[cfg(target_os = "macos")]
    {
        let src_c = CString::new(real_src.as_os_str().as_bytes())?;
        let dst_c = CString::new(dst.as_os_str().as_bytes())?;

        Retry::spawn(create_retry_strategy(), || async {
            match unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) } {
                0 => {
                    tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
                    Ok(())
                }
                _ => {
                    let _ = fs::remove_dir_all(dst).await.map_err(|e| {
                        tracing::debug!(
                            "Failed to clean target directory {}: {}",
                            dst.display(),
                            e
                        );
                    });
                    Err(anyhow::anyhow!(
                        "Failed to clone file: {}",
                        std::io::Error::last_os_error()
                    ))
                }
            }
        })
        .await?;
    }

    #[cfg(target_os = "linux")]
    {
        Retry::spawn(create_retry_strategy(), || async {
            linux_clone::clone_dir(&real_src, dst).await?;
            tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    }

    #[cfg(target_os = "windows")]
    {
        use super::retry::create_retry_strategy;
        use tokio_retry::Retry;

        Retry::spawn(create_retry_strategy(), || async {
            windows_clone::clone_dir(&real_src, dst).await?;
            tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
            Ok::<(), anyhow::Error>(())
        })
        .await?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        // For other platforms, use standard recursive copy
        copy_dir_all(&real_src, dst).await?;
        tracing::debug!("clone {} to {} success", real_src.display(), dst.display());
    }

    Ok(())
}

// Standard directory copy for non-macOS/Linux/Windows platforms
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).await?;
    let mut entries = fs::read_dir(src).await?;

    while let Some(entry) = entries.next_entry().await? {
        let ty = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            Box::pin(copy_dir_all(&src_path, &dst_path)).await?;
        } else {
            fs::copy(&src_path, &dst_path).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

    async fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> Result<PathBuf> {
        let path = dir.join(name);
        let mut file = fs::File::create(&path).await?;
        file.write_all(content).await?;
        Ok(path)
    }

    async fn create_test_structure(dir: &Path, structure: &[(&str, Option<&[u8]>)]) -> Result<()> {
        for (path, content) in structure {
            let full_path = dir.join(path);
            if let Some(content) = content {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                let mut file = fs::File::create(&full_path).await?;
                file.write_all(content).await?;
            } else {
                fs::create_dir_all(full_path).await?;
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_different_sizes() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(&src_dir, &[("file.txt", Some(b"content1"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"different"))]).await?;

        assert!(!validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_same_content() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(&src_dir, &[("file.txt", Some(b"same content"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"same content"))]).await?;

        assert!(validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_nested_structure() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(
            &src_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir1/dir2/file2.txt", Some(b"content2")),
                ("dir3/file3.txt", Some(b"content3")),
            ],
        )
        .await?;

        create_test_structure(
            &dst_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir1/dir2/file2.txt", Some(b"content2")),
                ("dir3/file3.txt", Some(b"content3")),
            ],
        )
        .await?;

        assert!(validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_validate_directory_different_structure() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        create_test_structure(
            &src_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir2/file2.txt", Some(b"content2")),
            ],
        )
        .await?;

        create_test_structure(
            &dst_dir,
            &[
                ("dir1/file1.txt", Some(b"content1")),
                ("dir3/file3.txt", Some(b"content31")),
            ],
        )
        .await?;

        assert!(!validate_directory(&src_dir, &dst_dir).await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_find_real_src() -> Result<()> {
        let temp = TempDir::new()?;
        let dir = temp.path().join("test_dir");
        fs::create_dir(&dir).await?;

        assert!(find_real_src(&dir).await.is_none());

        create_test_file(&dir, "file.txt", b"content").await?;
        assert!(find_real_src(&dir).await.is_none());

        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).await?;
        assert_eq!(find_real_src(&dir).await.unwrap(), subdir);

        Ok(())
    }

    #[tokio::test]
    async fn test_find_real_src_with_built_dir() -> Result<()> {
        let temp = TempDir::new()?;
        let dir = temp.path().join("test_dir");
        fs::create_dir(&dir).await?;

        // Create .utoo_built directory
        let built_dir = dir.join(".utoo_built");
        fs::create_dir(&built_dir).await?;

        // Create a regular subdirectory
        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).await?;

        assert_eq!(find_real_src(&dir).await.unwrap(), subdir);
        Ok(())
    }

    #[tokio::test]
    async fn test_clone_without_find_real() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        // Create source structure
        create_test_structure(
            &src_dir,
            &[
                (".utoo_built", None),
                ("real_dir/file.txt", Some(b"content")),
            ],
        )
        .await?;

        // Test cloning with find_real=false
        clone(&src_dir, &dst_dir, false).await?;

        // Verify everything was cloned
        assert!(dst_dir.join("real_dir").exists());
        assert!(dst_dir.join(".utoo_built").exists());
        assert_eq!(
            fs::read_to_string(dst_dir.join("real_dir/file.txt")).await?,
            "content"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_clone_existing_directory() -> Result<()> {
        let temp = TempDir::new()?;
        let src_dir = temp.path().join("src");
        let dst_dir = temp.path().join("dst");

        // Create initial source and destination
        create_test_structure(&src_dir, &[("file.txt", Some(b"old content"))]).await?;
        create_test_structure(&dst_dir, &[("file.txt", Some(b"old content"))]).await?;

        // Clone should succeed and skip since content is identical
        clone(&src_dir, &dst_dir, false).await?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("file.txt")).await?,
            "old content"
        );

        // Update source content
        create_test_structure(&src_dir, &[("file.txt", Some(b"content changed"))]).await?;

        // Clone should update the destination
        clone(&src_dir, &dst_dir, false).await?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("file.txt")).await?,
            "content changed"
        );

        Ok(())
    }

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::*;
        use tokio::fs::File;
        use tokio::io::AsyncReadExt;

        #[tokio::test]
        async fn test_clone_dir_basic() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory structure
            create_test_structure(
                &src_dir,
                &[
                    ("file1.txt", Some(b"content1")),
                    ("file2.txt", Some(b"content2")),
                    ("subdir", None),
                    ("subdir/file3.txt", Some(b"content3")),
                ],
            )
            .await?;

            // Perform clone operation
            linux_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify clone result
            assert!(validate_directory(&src_dir, &dst_dir).await?);

            // Verify file contents
            let mut content = String::new();
            File::open(dst_dir.join("file1.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "content1");

            content.clear();
            File::open(dst_dir.join("subdir/file3.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "content3");

            Ok(())
        }

        #[tokio::test]
        async fn test_clone_dir_nested() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create multi-level nested directory structure
            create_test_structure(
                &src_dir,
                &[
                    ("dir1", None),
                    ("dir1/dir2", None),
                    ("dir1/dir2/dir3", None),
                    ("dir1/dir2/dir3/file.txt", Some(b"deep content")),
                ],
            )
            .await?;

            // Perform clone operation
            linux_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify clone result
            assert!(validate_directory(&src_dir, &dst_dir).await?);

            // Verify deep file content
            let mut content = String::new();
            File::open(dst_dir.join("dir1/dir2/dir3/file.txt"))
                .await?
                .read_to_string(&mut content)
                .await?;
            assert_eq!(content, "deep content");

            Ok(())
        }

        #[tokio::test]
        async fn test_clone_dir_error_cases() -> Result<()> {
            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Test case when source directory doesn't exist
            let result = linux_clone::clone_dir(&src_dir, &dst_dir).await;
            assert!(result.is_err());
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );

            // Test case when source path is a file instead of a directory
            create_test_file(temp.path(), "not_a_dir", b"content").await?;
            let result =
                linux_clone::clone_dir(temp.path().join("not_a_dir").as_ref(), &dst_dir).await;
            assert!(result.is_err());

            Ok(())
        }

        #[tokio::test]
        async fn test_fast_copy_preserves_permissions() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory with an executable file
            fs::create_dir(&src_dir).await?;
            let src_file = src_dir.join("executable.sh");
            fs::write(&src_file, b"#!/bin/bash\necho 'test'").await?;

            // Set executable permissions (0o755)
            let mut perms = fs::metadata(&src_file).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&src_file, perms).await?;

            // Verify source file has correct permissions
            let src_metadata = fs::metadata(&src_file).await?;
            assert_eq!(src_metadata.permissions().mode() & 0o777, 0o755);

            // Use clone_dir to trigger fast_copy_file path (no _hasInstallScript flag)
            linux_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify destination file has same permissions
            let dst_file = dst_dir.join("executable.sh");
            let dst_metadata = fs::metadata(&dst_file).await?;
            assert_eq!(
                dst_metadata.permissions().mode() & 0o777,
                0o755,
                "Destination file should preserve executable permissions"
            );

            Ok(())
        }

        #[tokio::test]
        async fn test_fast_copy_preserves_read_only_permissions() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create source directory with a read-only file
            fs::create_dir(&src_dir).await?;
            let src_file = src_dir.join("readonly.txt");
            fs::write(&src_file, b"read only content").await?;

            // Set read-only permissions (0o444)
            let mut perms = fs::metadata(&src_file).await?.permissions();
            perms.set_mode(0o444);
            fs::set_permissions(&src_file, perms).await?;

            // Verify source file has correct permissions
            let src_metadata = fs::metadata(&src_file).await?;
            assert_eq!(src_metadata.permissions().mode() & 0o777, 0o444);

            // Use clone_dir to trigger fast_copy_file path
            linux_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify destination file has same permissions
            let dst_file = dst_dir.join("readonly.txt");
            let dst_metadata = fs::metadata(&dst_file).await?;
            assert_eq!(
                dst_metadata.permissions().mode() & 0o777,
                0o444,
                "Destination file should preserve read-only permissions"
            );

            Ok(())
        }

        #[tokio::test]
        async fn test_clone_dir_preserves_executable_in_subdirs() -> Result<()> {
            use std::os::unix::fs::PermissionsExt;

            let temp = TempDir::new()?;
            let src_dir = temp.path().join("src");
            let dst_dir = temp.path().join("dst");

            // Create directory structure with executable files
            create_test_structure(
                &src_dir,
                &[
                    ("bin", None),
                    ("bin/script.sh", Some(b"#!/bin/bash\necho 'hello'")),
                    ("lib", None),
                    ("lib/data.txt", Some(b"data")),
                ],
            )
            .await?;

            // Set executable permission on script.sh
            let script_path = src_dir.join("bin/script.sh");
            let mut perms = fs::metadata(&script_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).await?;

            // Set read-only permission on data.txt
            let data_path = src_dir.join("lib/data.txt");
            let mut perms = fs::metadata(&data_path).await?.permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&data_path, perms).await?;

            // Clone directory
            linux_clone::clone_dir(&src_dir, &dst_dir).await?;

            // Verify script.sh has executable permissions
            let dst_script = dst_dir.join("bin/script.sh");
            let dst_script_metadata = fs::metadata(&dst_script).await?;
            assert_eq!(
                dst_script_metadata.permissions().mode() & 0o777,
                0o755,
                "Executable file should preserve permissions in subdirectories"
            );

            // Verify data.txt has correct permissions
            let dst_data = dst_dir.join("lib/data.txt");
            let dst_data_metadata = fs::metadata(&dst_data).await?;
            assert_eq!(
                dst_data_metadata.permissions().mode() & 0o777,
                0o644,
                "Regular file should preserve permissions in subdirectories"
            );

            Ok(())
        }
    }
}
