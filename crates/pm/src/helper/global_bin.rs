use crate::util::platform_const::GLOBAL_NODE_MODULES;
use anyhow::{Context, Result};
use std::path::PathBuf;

// Get global bin directory based on prefix
//
// This function determines the global binary directory where executables should be linked.
// It's used by both link and install commands to ensure consistent binary linking behavior.
//
// # Arguments
// * `prefix` - Optional custom prefix path. If None, uses the current executable's directory.
//
// # Returns
// * `PathBuf` - Path to the global bin directory
//
// # Examples
//
// ```rust
// // Use default (current executable directory)
// let bin_dir = get_global_bin_dir(None)?;
//
// // Use custom prefix
// let bin_dir = get_global_bin_dir(Some("/usr/local"))?;
// ```

fn get_current_exe_dir() -> Result<PathBuf> {
    Ok(std::env::current_exe()
        .context("Failed to get current executable path")?
        .parent()
        .context("Failed to get executable parent directory")?
        .to_path_buf())
}

pub fn get_global_bin_dir(prefix: Option<&str>) -> Result<PathBuf> {
    match prefix {
        Some(prefix) => Ok(PathBuf::from(prefix).join("bin")),
        None => get_current_exe_dir(),
    }
}

// exp: /usr/local/lib/node_modules
pub fn get_global_package_dir(prefix: Option<&str>) -> Result<PathBuf> {
    let base_path = match prefix {
        Some(prefix) => PathBuf::from(prefix),
        None => get_current_exe_dir()?
            .parent()
            .context("Failed to get parent directory")?
            .to_path_buf(),
    };

    Ok(base_path.join(GLOBAL_NODE_MODULES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_global_bin_dir_with_prefix() {
        let prefix: Option<&str> = Some("/usr/local");
        let result = get_global_bin_dir(prefix).unwrap();
        assert_eq!(result, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn test_get_global_bin_dir_without_prefix() {
        let expected = std::env::current_exe()
            .expect("current_exe should exist")
            .parent()
            .expect("exe should have a parent directory")
            .to_path_buf();
        let result = get_global_bin_dir(None).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_get_global_bin_dir_with_empty_prefix() {
        let prefix: Option<&str> = Some("");
        let result = get_global_bin_dir(prefix).unwrap();
        assert_eq!(result, PathBuf::from("bin"));
    }

    #[test]
    fn test_get_global_package_dir_with_empty_prefix() {
        let prefix: Option<&str> = Some("");
        let result = get_global_package_dir(prefix).unwrap();
        assert_eq!(result, PathBuf::from(GLOBAL_NODE_MODULES));
    }
}
