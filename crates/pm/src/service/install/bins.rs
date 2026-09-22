//! Executable normalization and package bin links.
use crate::{fs, model::package::PackageInfo, util::linker::link};
use anyhow::{Context, Result};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn link_to_target(package: &PackageInfo, target_bin_dir: &Path) -> Result<()> {
    // Link each binary file
    for (bin_name, relative_path) in &package.bin_files {
        let target_path = package.path.join(relative_path);
        let link_path = target_bin_dir.join(bin_name);

        tracing::debug!("Linking global binary: {bin_name} -> {relative_path}");

        // Ensure target file is executable
        ensure_executable(&target_path)
            .await
            .context("Failed to ensure binary is executable")?;

        // Create symbolic link
        link(&target_path, &link_path)
            .await
            .context("Failed to create symbolic link")?;
    }

    Ok(())
}

pub async fn link_to_global(package: &PackageInfo, global_bin_dir: &Path) -> Result<()> {
    link_to_target(package, global_bin_dir).await?;

    Ok(())
}

pub async fn ensure_executable(target_path: &Path) -> Result<()> {
    // Early check for file existence (works on all platforms)
    let metadata = crate::fs::metadata(&target_path)
        .await
        .with_context(|| format!("Failed to access file {}", target_path.display()))?;

    if !metadata.is_file() {
        anyhow::bail!("Path is not a file: {}", target_path.display());
    }

    // Unix: only process if not already executable. Windows: always (no
    // executable bit to gate on).
    #[cfg(unix)]
    let needs_shebang = metadata.permissions().mode() & 0o111 == 0;
    #[cfg(not(unix))]
    let needs_shebang = true;

    if needs_shebang {
        try_add_shebang(target_path).await;
    }

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        let mut perms = crate::fs::metadata(&target_path)
            .await
            .with_context(|| format!("Failed to get file permissions {}", target_path.display()))?
            .permissions();

        perms.set_mode(0o755);
        fs::set_permissions(&target_path, perms)
            .await
            .context("Failed to set executable permissions")?;
    }

    Ok(())
}

/// Run [`check_and_add_shebang`](check_and_add_shebang) and log the
/// outcome. A shebang failure (binary / non-UTF8 file) is non-fatal — the
/// file just isn't a shell script — so it is logged, not propagated.
async fn try_add_shebang(target_path: &Path) {
    match check_and_add_shebang(target_path).await {
        Ok(true) => tracing::debug!("Added shebang to {}", target_path.display()),
        Ok(false) => {}
        Err(e) => tracing::debug!("Skipping shebang for {}: {}", target_path.display(), e),
    }
}

/// Check if file needs shebang and add it if needed
/// Returns Ok(true) if shebang was added, Ok(false) if not needed, Err if binary/non-UTF8
async fn check_and_add_shebang(target_path: &Path) -> Result<bool> {
    // Read first 512 bytes to check for shebang and validate UTF-8
    // file is automatically dropped here
    let header = {
        let mut file = fs::File::open(target_path).await?;
        let mut buffer = vec![0u8; 512];
        let n = file.read(&mut buffer).await?;
        buffer.truncate(n);

        // Try to parse as UTF-8 to detect binary files early
        std::str::from_utf8(&buffer)
            .map_err(|_| anyhow::anyhow!("File is not valid UTF-8, likely a binary file"))?
            .to_string()
    };

    // Check if already has shebang
    if header.starts_with("#!") {
        return Ok(false);
    }

    // Need to add shebang - read entire file now
    let content = fs::read_to_string(target_path).await?;
    let new_content = format!("#!/usr/bin/env node\n{}", content);

    // Write the modified content
    // file is automatically dropped here
    {
        let mut file = fs::File::create(target_path).await?;
        file.write_all(new_content.as_bytes()).await?;
        file.flush().await?;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    #[tokio::test]
    async fn test_ensure_executable() {
        // Create a temporary directory
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.sh");
        fs::write(&test_file, "#!/bin/sh\necho test").unwrap();

        // Test ensure_executable
        let result = ensure_executable(&test_file).await;
        assert!(result.is_ok(), "Failed to ensure file is executable");

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&test_file).unwrap().permissions();
            assert!(permissions.mode() & 0o111 != 0, "File not made executable");
        }
    }

    #[tokio::test]
    async fn test_ensure_executable_nonexistent_file() {
        // Test with non-existent file
        let result = ensure_executable(Path::new("nonexistent-file")).await;
        assert!(result.is_err(), "Should fail with non-existent file");
    }

    #[tokio::test]
    async fn test_ensure_executable_binary_file() {
        // Test with a binary file (simulating node executable)
        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("node");

        // Create a fake binary file with non-UTF8 bytes
        let binary_data = vec![
            0x7f, 0x45, 0x4c, 0x46, // ELF magic number
            0x02, 0x01, 0x01, 0x00, // 64-bit, little-endian
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFE, 0xFD,
            0xFC, // Some non-UTF8 bytes
        ];
        fs::write(&binary_file, &binary_data).unwrap();

        // Should not fail, just skip shebang and set permissions
        let result = ensure_executable(&binary_file).await;
        assert!(result.is_ok(), "Should handle binary files gracefully");

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&binary_file).unwrap().permissions();
            assert!(
                permissions.mode() & 0o111 != 0,
                "Binary file should be executable"
            );
        }

        // File content should not be modified
        let content = fs::read(&binary_file).unwrap();
        assert_eq!(content, binary_data, "Binary file should not be modified");
    }

    #[tokio::test]
    async fn test_ensure_executable_text_without_shebang() {
        // Test with a text file without shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.js");

        // Create a text file without shebang
        fs::write(&text_file, "console.log('hello');").unwrap();

        let result = ensure_executable(&text_file).await;
        assert!(result.is_ok(), "Should add shebang to text file");

        // Should have shebang added
        let content = fs::read_to_string(&text_file).unwrap();
        assert!(
            content.starts_with("#!/usr/bin/env node\n"),
            "Shebang should be added"
        );
        assert!(
            content.contains("console.log('hello');"),
            "Original content should be preserved"
        );

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&text_file).unwrap().permissions();
            assert!(
                permissions.mode() & 0o111 != 0,
                "Text file should be executable"
            );
        }
    }

    #[tokio::test]
    async fn test_ensure_executable_text_with_shebang() {
        // Test with a text file that already has shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.sh");

        let original_content = "#!/bin/bash\necho 'test'";
        fs::write(&text_file, original_content).unwrap();

        let result = ensure_executable(&text_file).await;
        assert!(result.is_ok(), "Should handle file with existing shebang");

        // Content should not be modified
        let content = fs::read_to_string(&text_file).unwrap();
        assert_eq!(
            content, original_content,
            "File with shebang should not be modified"
        );

        #[cfg(unix)]
        {
            let permissions = fs::metadata(&text_file).unwrap().permissions();
            assert!(permissions.mode() & 0o111 != 0, "File should be executable");
        }
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_binary() {
        // Test check_and_add_shebang with binary file
        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("binary");

        // Create binary file
        let binary_data = vec![0xFF, 0xFE, 0xFD, 0xFC];
        fs::write(&binary_file, &binary_data).unwrap();

        let result = check_and_add_shebang(&binary_file).await;
        assert!(result.is_err(), "Should return error for binary file");
        assert!(
            result.unwrap_err().to_string().contains("UTF-8"),
            "Error should mention UTF-8"
        );
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_text_without_shebang() {
        // Test check_and_add_shebang with text file without shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.js");

        fs::write(&text_file, "console.log('test');").unwrap();

        let result = check_and_add_shebang(&text_file).await;
        assert!(result.is_ok(), "Should succeed for text file");
        assert!(result.unwrap(), "Should return true when shebang was added");

        let content = fs::read_to_string(&text_file).unwrap();
        assert!(content.starts_with("#!/usr/bin/env node\n"));
    }

    #[tokio::test]
    async fn test_check_and_add_shebang_text_with_shebang() {
        // Test check_and_add_shebang with text file that already has shebang
        let temp_dir = TempDir::new().unwrap();
        let text_file = temp_dir.path().join("script.sh");

        fs::write(&text_file, "#!/bin/sh\necho test").unwrap();

        let result = check_and_add_shebang(&text_file).await;
        assert!(result.is_ok(), "Should succeed for file with shebang");
        assert!(
            !result.unwrap(),
            "Should return false when shebang already exists"
        );
    }
}
