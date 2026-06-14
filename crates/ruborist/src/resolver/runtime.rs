//! Runtime dependency injection for Node.js.
//!
//! This module handles automatic injection of Node.js runtime packages
//! when `engines.install-node` is specified in package.json.
//!
//! # Example
//!
//! When package.json contains:
//! ```json
//! {
//!   "engines": {
//!     "install-node": "16.17.0"
//!   }
//! }
//! ```
//!
//! This will inject optional dependencies for node-bin packages:
//! - `node-darwin-x64`
//! - `node-bin-darwin-arm64`
//! - `node-linux-x64`
//! - `node-linux-arm64`
//! - `node-win-x64`
//! - `node-win-x86`

use std::collections::HashMap;

// Native implementation
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    /// Platform to supported architectures mapping.
    ///
    /// Format: (prefix, platform, architectures)
    /// - `node-darwin-arm64` is not available for node < 16
    /// - `node-bin-darwin-arm64` is used instead for ARM64 Macs
    const PLATFORM_ARCHS: &[(&str, &str, &[&str])] = &[
        ("node", "darwin", &["x64"]),
        ("node-bin", "darwin", &["arm64"]),
        ("node", "linux", &["x64", "arm64"]),
        ("node", "win", &["x64", "x86"]),
    ];

    /// Generate node-bin optional dependencies for all supported platforms.
    fn get_node_deps(version: &str) -> HashMap<String, String> {
        let mut optional_deps = HashMap::new();

        for (prefix, platform, archs) in PLATFORM_ARCHS {
            for arch in *archs {
                let dep_name = format!("{prefix}-{platform}-{arch}");
                optional_deps.insert(dep_name, version.to_string());
            }
        }

        optional_deps
    }

    pub fn install_runtime_from_map(engines: &HashMap<String, String>) -> HashMap<String, String> {
        let version = match engines.get("install-node") {
            Some(v) if !v.is_empty() => v.as_str(),
            _ => return HashMap::new(),
        };

        get_node_deps(version)
    }
}

// Wasm implementation - no runtime injection needed
#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;

    pub fn install_runtime_from_map(_engines: &HashMap<String, String>) -> HashMap<String, String> {
        HashMap::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_runtime_from_map_with_valid_version() {
        let mut engines = HashMap::new();
        engines.insert("install-node".to_string(), "18.0.0".to_string());

        let result = install_runtime_from_map(&engines);

        // Check all expected per-platform dependencies.
        assert_eq!(result.get("node-darwin-x64"), Some(&"18.0.0".to_string()));
        assert_eq!(
            result.get("node-bin-darwin-arm64"),
            Some(&"18.0.0".to_string())
        );
        assert_eq!(result.get("node-linux-x64"), Some(&"18.0.0".to_string()));
        assert_eq!(result.get("node-linux-arm64"), Some(&"18.0.0".to_string()));
        assert_eq!(result.get("node-win-x64"), Some(&"18.0.0".to_string()));
        assert_eq!(result.get("node-win-x86"), Some(&"18.0.0".to_string()));
        assert_eq!(result.len(), 6);
    }

    #[test]
    fn test_install_runtime_from_map_empty() {
        let engines = HashMap::new();
        let result = install_runtime_from_map(&engines);
        assert!(result.is_empty());
    }

    #[test]
    fn test_install_runtime_from_map_empty_version() {
        let mut engines = HashMap::new();
        engines.insert("install-node".to_string(), "".to_string());

        let result = install_runtime_from_map(&engines);
        assert!(result.is_empty());
    }
}
