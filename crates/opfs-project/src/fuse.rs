use crate::util::get_package_name;
use anyhow::Result;

/// Read file content with fuse.link support
pub async fn read(path: &str) -> Result<Vec<u8>> {
    let prepared_path = crate::util::prepare_path(path);

    // Try to read through node_modules fuse link logic first
    if let Some(content) = try_read_through_fuse_link(&prepared_path).await? {
        return Ok(content);
    }

    // Fallback to direct read
    let content = tokio_fs_ext::read(&prepared_path).await?;
    Ok(content)
}

/// Read directory contents with file type information and fuse.link support
pub async fn read_dir(path: &str) -> Result<Vec<crate::DirEntry>> {
    let prepared_path = crate::util::prepare_path(path);

    // Handle node_modules fuse.link logic
    if let Some(entries) = try_read_dir_through_fuse_link(&prepared_path).await? {
        return Ok(entries);
    }

    // Handle direct directory reading
    let entries = crate::util::read_dir_direct(&prepared_path).await?;

    // Handle single fuse.link file case
    if let Some(entries) = try_read_dir_through_single_fuse_link(&prepared_path, &entries).await? {
        return Ok(entries);
    }

    Ok(entries)
}

/// Create fuse link between source and destination directories
pub async fn fuse_link(src: &str, dst: &str) -> Result<()> {
    // Create the destination directory if it doesn't exist
    tokio_fs_ext::create_dir_all(dst).await?;

    let link_file_path = format!("{dst}/fuse.link");

    // Check if fuse.link already exists
    if tokio_fs_ext::metadata(&link_file_path).await.is_ok() {
        // Read existing content
        let existing_content = tokio_fs_ext::read_to_string(&link_file_path).await?;
        let mut links: Vec<String> = existing_content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|s| s.to_string())
            .collect();

        // Add new link if not already present
        if !links.contains(&src.to_string()) {
            links.push(src.to_string());
        }

        // Write back all links
        let new_content = links.join("\n") + "\n";
        tokio_fs_ext::write(&link_file_path, new_content.as_bytes()).await?;
    } else {
        // Create new fuse.link file with the source path
        let link_content = format!("{src}\n");
        tokio_fs_ext::write(&link_file_path, link_content.as_bytes()).await?;
    }

    Ok(())
}

/// Get fuse.link path for a given path that contains node_modules
fn get_fuse_link_path(path: &str) -> Option<String> {
    if let Some(package_name) = get_package_name(path) {
        // Find node_modules in the path
        if let Some(node_modules_pos) = path.find("node_modules") {
            // Construct the path: original_path_up_to_node_modules/node_modules/package_name/fuse.link
            let before_node_modules = &path[..node_modules_pos];
            return Some(format!(
                "{before_node_modules}/node_modules/{package_name}/fuse.link"
            ));
        }
    }
    None
}

/// Try to read file through fuse link logic for node_modules
async fn try_read_through_fuse_link(prepared_path: &str) -> Result<Option<Vec<u8>>> {
    if !prepared_path.contains("node_modules") {
        return Ok(None);
    }

    let fuse_link_path = match get_fuse_link_path(prepared_path) {
        Some(path) => path,
        None => return Ok(None),
    };

    let link_content = match tokio_fs_ext::read_to_string(&fuse_link_path).await {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let target_dir = link_content.lines().next().unwrap_or("").trim();
    if target_dir.is_empty() {
        return Ok(None);
    }

    let relative_path = extract_relative_path_from_node_modules(prepared_path)?;
    let target_path = format!("{target_dir}/{relative_path}");

    match tokio_fs_ext::read(&target_path).await {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

/// Extract relative path from node_modules path
fn extract_relative_path_from_node_modules(prepared_path: &str) -> Result<String> {
    let package_name = get_package_name(prepared_path)
        .ok_or_else(|| anyhow::anyhow!("Could not extract package name"))?;

    let node_modules_pos = prepared_path
        .find("node_modules")
        .ok_or_else(|| anyhow::anyhow!("Could not find node_modules in path"))?;

    // Get the part after node_modules/package_name/
    let after_package = &prepared_path[node_modules_pos + "node_modules".len()..];
    let after_package = after_package.trim_start_matches('/');

    // Remove the package name from the path
    let relative_path = if after_package.starts_with(&package_name) {
        after_package[package_name.len()..]
            .trim_start_matches('/')
            .to_string()
    } else {
        // Fallback: just get the filename
        std::path::Path::new(prepared_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    Ok(relative_path)
}

/// Try to read directory through fuse.link for node_modules
async fn try_read_dir_through_fuse_link(
    prepared_path: &str,
) -> Result<Option<Vec<crate::DirEntry>>> {
    if !prepared_path.contains("node_modules") {
        return Ok(None);
    }

    let fuse_link_path = match get_fuse_link_path(prepared_path) {
        Some(path) => path,
        None => return Ok(None),
    };

    let link_content = match tokio_fs_ext::read_to_string(&fuse_link_path).await {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let target_dir = link_content.lines().next().unwrap_or("").trim();
    if target_dir.is_empty() {
        return Ok(None);
    }

    let target_path = get_target_path_for_node_modules(prepared_path, target_dir)?;
    let entries = crate::util::read_dir_direct(&target_path).await?;
    Ok(Some(entries))
}

/// Get target path for node_modules directory
fn get_target_path_for_node_modules(prepared_path: &str, target_dir: &str) -> Result<String> {
    let package_name = get_package_name(prepared_path)
        .ok_or_else(|| anyhow::anyhow!("Could not extract package name"))?;

    let dir_name = std::path::Path::new(prepared_path)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Could not get directory name"))?;

    let dir_name_str = dir_name.to_string_lossy();

    // Check if directory name matches package name
    if dir_name_str == package_name {
        // Case 1: directory name matches package name (e.g., node_modules/lodash)
        Ok(target_dir.to_string())
    } else {
        // Case 2: directory name doesn't match package name (e.g., node_modules/@lodash/has)
        Ok(format!("{target_dir}/{dir_name_str}"))
    }
}

/// Try to read directory through single fuse.link file
async fn try_read_dir_through_single_fuse_link(
    prepared_path: &str,
    entries: &[crate::DirEntry],
) -> Result<Option<Vec<crate::DirEntry>>> {
    // Check if entries only contains one entry and it is fuse.link
    if entries.len() != 1 || entries[0].name != "fuse.link" {
        return Ok(None);
    }

    let link_file_path = format!("{prepared_path}/fuse.link");
    let link_content = tokio_fs_ext::read_to_string(&link_file_path).await?;
    let target_dir = link_content.lines().next().unwrap_or("").trim();

    if target_dir.is_empty() {
        return Ok(None);
    }

    let entries = crate::util::read_dir_direct(target_dir).await?;
    Ok(Some(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DirEntry;
    use tempfile::TempDir;

    /// Test helper: create a temporary directory with test files
    async fn create_test_dir() -> (TempDir, String) {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_string_lossy().to_string();

        // Create test files
        tokio_fs_ext::write(&format!("{}/test.txt", temp_path), b"Hello, World!")
            .await
            .unwrap();

        tokio_fs_ext::write(
            &format!("{}/package.json", temp_path),
            b"{\"name\": \"test-package\"}",
        )
        .await
        .unwrap();

        (temp_dir, temp_path)
    }

    /// Test helper: create a node_modules structure
    async fn create_node_modules_structure(base_path: &str, package_name: &str) -> String {
        let node_modules_path = format!("{}/node_modules", base_path);
        let package_path = format!("{}/{}", node_modules_path, package_name);

        tokio_fs_ext::create_dir_all(&package_path).await.unwrap();

        // Create fuse.link file
        let fuse_link_content = format!("{}\n", base_path);
        tokio_fs_ext::write(
            &format!("{}/fuse.link", package_path),
            fuse_link_content.as_bytes(),
        )
        .await
        .unwrap();

        package_path
    }

    #[tokio::test]
    async fn test_fuse_link_creation() {
        let temp_dir = TempDir::new().unwrap();
        let src_path = "/tmp/source";
        let dst_path = format!("{}/destination", temp_dir.path().to_string_lossy());

        // Create source directory
        tokio_fs_ext::create_dir_all(src_path).await.unwrap();
        tokio_fs_ext::write(&format!("{}/test.txt", src_path), b"Source content")
            .await
            .unwrap();

        // Create fuse link
        let result = fuse_link(src_path, &dst_path).await;
        assert!(result.is_ok());

        // Verify fuse.link file was created
        let link_file_path = format!("{}/fuse.link", dst_path);
        let link_content = tokio_fs_ext::read_to_string(&link_file_path).await.unwrap();
        assert_eq!(link_content.trim(), src_path);
    }

    #[tokio::test]
    async fn test_fuse_link_multiple_sources() {
        let temp_dir = TempDir::new().unwrap();
        let dst_path = format!("{}/destination", temp_dir.path().to_string_lossy());

        // Create first fuse link
        fuse_link("/tmp/source1", &dst_path).await.unwrap();

        // Add second source
        let result = fuse_link("/tmp/source2", &dst_path).await;
        assert!(result.is_ok());

        // Verify both sources are in fuse.link
        let link_file_path = format!("{}/fuse.link", dst_path);
        let link_content = tokio_fs_ext::read_to_string(&link_file_path).await.unwrap();
        let lines: Vec<&str> = link_content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&"/tmp/source1"));
        assert!(lines.contains(&"/tmp/source2"));
    }

    #[tokio::test]
    async fn test_get_fuse_link_path() {
        let test_cases = vec![
            (
                "/path/to/node_modules/lodash/index.js",
                Some("/path/to/node_modules/lodash/fuse.link"),
            ),
            (
                "/path/to/node_modules/@types/node/index.d.ts",
                Some("/path/to/node_modules/@types/node/fuse.link"),
            ),
            ("/path/to/some/file.txt", None),
            ("/path/to/node_modules/", None),
        ];

        for (input, expected) in test_cases {
            let result = get_fuse_link_path(input);
            // Normalize path separators for comparison
            let normalized_result = result.map(|s| s.replace("//", "/"));
            let normalized_expected = expected.map(|s| s.to_string().replace("//", "/"));
            assert_eq!(normalized_result, normalized_expected);
        }
    }

    #[tokio::test]
    async fn test_extract_relative_path_from_node_modules() {
        let test_cases = vec![
            ("/path/to/node_modules/lodash/index.js", "index.js"),
            ("/path/to/node_modules/@types/node/index.d.ts", "index.d.ts"),
            ("/path/to/node_modules/lodash/lib/array.js", "lib/array.js"),
            (
                "/path/to/node_modules/@types/node/types/index.d.ts",
                "types/index.d.ts",
            ),
        ];

        for (input, expected) in test_cases {
            let result = extract_relative_path_from_node_modules(input).unwrap();
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_extract_relative_path_from_node_modules_invalid() {
        let invalid_paths = vec![
            "/path/to/some/file.txt",
            "/path/to/node_modules/",
            "/path/to/node_modules",
        ];

        for path in invalid_paths {
            let result = extract_relative_path_from_node_modules(path);
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_read_through_fuse_link() {
        let (temp_dir, base_path) = create_test_dir().await;
        let package_name = "test-package";
        let package_path = create_node_modules_structure(&base_path, package_name).await;

        // Create a file in the source directory
        let source_file = format!("{}/source_file.txt", base_path);
        tokio_fs_ext::write(&source_file, b"Fuse link content")
            .await
            .unwrap();

        // Test reading through fuse link
        let node_modules_file = format!("{}/source_file.txt", package_path);
        let result = try_read_through_fuse_link(&node_modules_file)
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"Fuse link content");

        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_read_through_fuse_link_no_node_modules() {
        let result = try_read_through_fuse_link("/path/to/regular/file.txt")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_read_through_fuse_link_no_fuse_link_file() {
        let result = try_read_through_fuse_link("/path/to/node_modules/lodash/index.js")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_read_dir_through_fuse_link() {
        let (temp_dir, base_path) = create_test_dir().await;
        let package_name = "test-package";
        let package_path = create_node_modules_structure(&base_path, package_name).await;

        // Create a subdirectory in the source
        let source_subdir = format!("{}/subdir", base_path);
        tokio_fs_ext::create_dir_all(&source_subdir).await.unwrap();
        tokio_fs_ext::write(
            &format!("{}/subfile.txt", source_subdir),
            b"Subdirectory file",
        )
        .await
        .unwrap();

        // Test reading directory through fuse link
        let result = try_read_dir_through_fuse_link(&package_path).await.unwrap();

        assert!(result.is_some());
        let entries = result.unwrap();
        assert!(!entries.is_empty());

        // Verify we can find the test files
        let file_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        assert!(file_names.contains(&"test.txt".to_string()));
        assert!(file_names.contains(&"package.json".to_string()));
        assert!(file_names.contains(&"subdir".to_string()));

        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_read_dir_through_single_fuse_link() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = format!("{}/test_dir", temp_dir.path().to_string_lossy());
        let source_path = format!("{}/source", temp_dir.path().to_string_lossy());

        // Create test directory with only fuse.link file
        tokio_fs_ext::create_dir_all(&test_path).await.unwrap();
        tokio_fs_ext::write(
            &format!("{}/fuse.link", test_path),
            &format!("{}\n", source_path),
        )
        .await
        .unwrap();

        // Create source directory with some files
        tokio_fs_ext::create_dir_all(&source_path).await.unwrap();
        tokio_fs_ext::write(&format!("{}/file1.txt", source_path), b"File 1")
            .await
            .unwrap();
        tokio_fs_ext::write(&format!("{}/file2.txt", source_path), b"File 2")
            .await
            .unwrap();

        // Create mock entries
        let entries = vec![DirEntry {
            name: "fuse.link".to_string(),
            r#type: DirEntryType::File
        }];

        let result = try_read_dir_through_single_fuse_link(&test_path, &entries)
            .await
            .unwrap();

        assert!(result.is_some());
        let result_entries = result.unwrap();
        assert_eq!(result_entries.len(), 2);

        let file_names: Vec<String> = result_entries.iter().map(|e| e.name.clone()).collect();
        assert!(file_names.contains(&"file1.txt".to_string()));
        assert!(file_names.contains(&"file2.txt".to_string()));
    }

    #[tokio::test]
    async fn test_read_dir_through_single_fuse_link_multiple_entries() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = format!("{}/test_dir", temp_dir.path().to_string_lossy());

        // Create mock entries with multiple files
        let entries = vec![
            DirEntry {
                name: "fuse.link".to_string(),
                r#type: DirEntryType::File
            },
            DirEntry {
                name: "other.txt".to_string(),
                r#type: DirEntryType::File
            },
        ];

        let result = try_read_dir_through_single_fuse_link(&test_path, &entries)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_target_path_for_node_modules() {
        let test_cases = vec![
            ("/path/to/node_modules/lodash", "/tmp/source", "/tmp/source"),
            (
                "/path/to/node_modules/@types/node",
                "/tmp/source",
                "/tmp/source/node",
            ),
            (
                "/path/to/node_modules/lodash/lib",
                "/tmp/source",
                "/tmp/source/lib",
            ),
        ];

        for (prepared_path, target_dir, expected) in test_cases {
            let result = get_target_path_for_node_modules(prepared_path, target_dir).unwrap();
            // Normalize path separators for comparison
            let normalized_result = result.replace("//", "/");
            let normalized_expected = expected.replace("//", "/");
            assert_eq!(normalized_result, normalized_expected);
        }
    }

    #[tokio::test]
    async fn test_read_with_fuse_link() {
        let (temp_dir, base_path) = create_test_dir().await;
        let package_name = "test-package";
        let package_path = create_node_modules_structure(&base_path, package_name).await;

        // Create a file in the source directory
        let source_file = format!("{}/fuse_test.txt", base_path);
        tokio_fs_ext::write(&source_file, b"Fuse link test content")
            .await
            .unwrap();

        // Test reading through the main read function
        let node_modules_file = format!("{}/fuse_test.txt", package_path);
        let result = read(&node_modules_file).await.unwrap();

        assert_eq!(result, b"Fuse link test content");

        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_read_dir_with_fuse_link() {
        let (temp_dir, base_path) = create_test_dir().await;
        let package_name = "test-package";
        let package_path = create_node_modules_structure(&base_path, package_name).await;

        // Test reading directory through the main read_dir function
        let result = read_dir(&package_path).await.unwrap();

        assert!(!result.is_empty());
        let file_names: Vec<String> = result.iter().map(|e| e.name.clone()).collect();
        assert!(file_names.contains(&"test.txt".to_string()));
        assert!(file_names.contains(&"package.json".to_string()));

        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_read_regular_file() {
        let (temp_dir, base_path) = create_test_dir().await;

        // Test reading a regular file (not in node_modules)
        let result = read(&format!("{}/test.txt", base_path)).await.unwrap();
        assert_eq!(result, b"Hello, World!");

        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_read_dir_regular_directory() {
        let (temp_dir, base_path) = create_test_dir().await;

        // Test reading a regular directory
        let result = read_dir(&base_path).await.unwrap();

        assert!(!result.is_empty());
        let file_names: Vec<String> = result.iter().map(|e| e.name.clone()).collect();
        assert!(file_names.contains(&"test.txt".to_string()));
        assert!(file_names.contains(&"package.json".to_string()));

        drop(temp_dir);
    }
}
