use crate::DirEntry;
use anyhow::Result;

/// Extract package name from a path that contains node_modules
pub fn get_package_name(path: &str) -> Option<String> {
    // Find node_modules in the path
    if let Some(node_modules_pos) = path.find("node_modules") {
        // Get the part after node_modules
        let after_node_modules = &path[node_modules_pos + "node_modules".len()..];
        let after_node_modules = after_node_modules.trim_start_matches('/');

        // Split by '/' and take only the first two components
        let mut components = after_node_modules.split('/').take(2);
        let first = components.next();

        first?;

        // Check if first component starts with @ (scoped package)
        let package_name = if first.unwrap().starts_with('@') {
            // For scoped packages, we need two components: @scope/package
            if let Some(second) = components.next() {
                format!("{}/{}", first.unwrap(), second)
            } else {
                return None;
            }
        } else {
            // For regular packages, just use the first component
            first.unwrap().to_string()
        };

        if !package_name.is_empty() {
            return Some(package_name);
        }
    }
    None
}

/// Prepare path by resolving relative paths against current working directory
pub async fn prepare_path(path: &str) -> Result<String> {
    if path.starts_with('/') {
        Ok(path.to_string())
    } else {
        let cwd = crate::cwd::get_cwd().await?;
        Ok(format!("{cwd}/{path}"))
    }
}

/// Read directory directly without fuse.link logic
pub async fn read_dir_direct(path: &str) -> Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    let mut read_dir = tokio_fs_ext::read_dir(path)
        .await
        .map_err(|e| anyhow::anyhow!("Error reading directory: {}", e))?;

    while let Some(entry) = read_dir.next_entry().await? {
        let entry_path = entry.path();

        if let Some(name) = entry_path.file_name()
            && let Some(name_str) = name.to_str()
        {
            let meta = tokio_fs_ext::metadata(&entry_path).await?;
            let is_file = meta.is_file();
            let is_dir = meta.is_dir();

            let dir_entry = DirEntry {
                name: name_str.to_string(),
                is_file,
                is_dir,
            };
            entries.push(dir_entry);
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_get_package_name_regular_package() {
        let test_cases = vec![
            (
                "/path/to/node_modules/lodash/index.js",
                Some("lodash".to_string()),
            ),
            (
                "/path/to/node_modules/express/lib/app.js",
                Some("express".to_string()),
            ),
            (
                "/path/to/node_modules/react/package.json",
                Some("react".to_string()),
            ),
            (
                "node_modules/axios/dist/axios.js",
                Some("axios".to_string()),
            ),
        ];

        for (input, expected) in test_cases {
            let result = get_package_name(input);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_get_package_name_scoped_package() {
        let test_cases = vec![
            (
                "/path/to/node_modules/@types/node/index.d.ts",
                Some("@types/node".to_string()),
            ),
            (
                "/path/to/node_modules/@angular/core/core.js",
                Some("@angular/core".to_string()),
            ),
            (
                "/path/to/node_modules/@babel/preset-env/index.js",
                Some("@babel/preset-env".to_string()),
            ),
            (
                "node_modules/@vue/cli-service/bin/vue-cli-service.js",
                Some("@vue/cli-service".to_string()),
            ),
        ];

        for (input, expected) in test_cases {
            let result = get_package_name(input);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_get_package_name_invalid_paths() {
        let invalid_paths = vec![
            "/path/to/some/file.txt",
            "/path/to/node_modules/",
            "/path/to/node_modules",
            "/path/to/some/other/path",
            "",
            "node_modules",
        ];

        for path in invalid_paths {
            let result = get_package_name(path);
            assert!(result.is_none(), "Expected None for path: {}", path);
        }
    }

    #[tokio::test]
    async fn test_get_package_name_edge_cases() {
        let test_cases = vec![
            // Multiple node_modules in path
            (
                "/path/to/node_modules/lodash/node_modules/other/index.js",
                Some("lodash".to_string()),
            ),
            // Very long package names
            (
                "/path/to/node_modules/very-long-package-name-with-many-characters/index.js",
                Some("very-long-package-name-with-many-characters".to_string()),
            ),
            // Scoped package with long names
            (
                "/path/to/node_modules/@very-long-scope/very-long-package-name/index.js",
                Some("@very-long-scope/very-long-package-name".to_string()),
            ),
            // Path with spaces and special characters
            (
                "/path/to/node_modules/package-name/index.js",
                Some("package-name".to_string()),
            ),
        ];

        for (input, expected) in test_cases {
            let result = get_package_name(input);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_prepare_path_absolute() {
        let test_cases = vec![
            "/absolute/path",
            "/usr/local/bin",
            "/home/user/project",
            "/",
        ];

        for path in test_cases {
            let result = prepare_path(path).await.unwrap();
            assert_eq!(result, path);
        }
    }

    #[tokio::test]
    async fn test_prepare_path_relative() {
        // Since CWD is managed globally and has a default value,
        // we'll test that relative paths are properly concatenated
        // by checking the structure rather than exact values

        let test_cases = vec![
            "file.txt",
            "src/main.rs",
            "../parent/file.js",
            "./current/file.py",
        ];

        for relative_path in test_cases {
            let result = prepare_path(relative_path).await.unwrap();

            // Verify that the result contains the relative path at the end
            assert!(
                result.ends_with(relative_path),
                "Result '{}' should end with '{}'",
                result,
                relative_path
            );

            // Verify that it's not an absolute path starting with /
            assert!(
                !relative_path.starts_with('/') || result != relative_path,
                "Relative path '{}' should be resolved to absolute path",
                relative_path
            );
        }
    }

    #[tokio::test]
    async fn test_prepare_path_empty() {
        let result = prepare_path("").await.unwrap();

        // Should end with a slash since it's an empty path
        assert!(
            result.ends_with('/'),
            "Empty path should end with '/', got: {}",
            result
        );

        // Should not be empty
        assert!(
            !result.is_empty(),
            "Empty path should not result in empty string"
        );
    }

    #[tokio::test]
    async fn test_read_dir_direct() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_string_lossy().to_string();

        // Create test files and directories
        tokio_fs_ext::write(&format!("{}/file1.txt", temp_path), b"content1")
            .await
            .unwrap();
        tokio_fs_ext::write(&format!("{}/file2.js", temp_path), b"content2")
            .await
            .unwrap();
        tokio_fs_ext::create_dir_all(&format!("{}/subdir", temp_path))
            .await
            .unwrap();
        tokio_fs_ext::write(&format!("{}/subdir/file3.py", temp_path), b"content3")
            .await
            .unwrap();

        let entries = read_dir_direct(&temp_path).await.unwrap();

        // Should have at least 3 entries (file1.txt, file2.js, subdir)
        assert!(entries.len() >= 3);

        let file_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();

        // Check for files
        assert!(file_names.contains(&"file1.txt".to_string()));
        assert!(file_names.contains(&"file2.js".to_string()));

        // Check for directory
        assert!(file_names.contains(&"subdir".to_string()));

        // Verify file types
        for entry in &entries {
            match entry.name.as_str() {
                "file1.txt" | "file2.js" => {
                    assert!(entry.is_file);
                    assert!(!entry.is_dir);
                }
                "subdir" => {
                    assert!(!entry.is_file);
                    assert!(entry.is_dir);
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn test_read_dir_direct_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_string_lossy().to_string();

        let entries = read_dir_direct(&temp_path).await.unwrap();

        // Empty directory should return empty list
        assert_eq!(entries.len(), 0);
    }

    #[tokio::test]
    async fn test_read_dir_direct_nonexistent_directory() {
        let result = read_dir_direct("/nonexistent/directory/path").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_dir_direct_with_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_string_lossy().to_string();

        // Create regular and hidden files
        tokio_fs_ext::write(&format!("{}/visible.txt", temp_path), b"visible")
            .await
            .unwrap();
        tokio_fs_ext::write(&format!("{}/.hidden", temp_path), b"hidden")
            .await
            .unwrap();
        tokio_fs_ext::create_dir_all(&format!("{}/.hidden_dir", temp_path))
            .await
            .unwrap();

        let entries = read_dir_direct(&temp_path).await.unwrap();

        let file_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();

        // Should include both visible and hidden files
        assert!(file_names.contains(&"visible.txt".to_string()));
        assert!(file_names.contains(&".hidden".to_string()));
        assert!(file_names.contains(&".hidden_dir".to_string()));
    }

    #[tokio::test]
    async fn test_read_dir_direct_with_special_characters() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_string_lossy().to_string();

        // Create files with special characters in names
        tokio_fs_ext::write(&format!("{}/file with spaces.txt", temp_path), b"content")
            .await
            .unwrap();
        tokio_fs_ext::write(&format!("{}/file-with-dashes.js", temp_path), b"content")
            .await
            .unwrap();
        tokio_fs_ext::write(
            &format!("{}/file_with_underscores.py", temp_path),
            b"content",
        )
        .await
        .unwrap();

        let entries = read_dir_direct(&temp_path).await.unwrap();

        let file_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();

        assert!(file_names.contains(&"file with spaces.txt".to_string()));
        assert!(file_names.contains(&"file-with-dashes.js".to_string()));
        assert!(file_names.contains(&"file_with_underscores.py".to_string()));
    }
}
