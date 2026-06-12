use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Read and parse a JSON file into the specified type.
pub async fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = crate::fs::read(path)
        .await
        .with_context(|| format!("Failed to read file {path:?}"))?;

    serde_json::from_slice(&bytes).with_context(|| format!("Failed to parse JSON from {path:?}"))
}

/// Serialize `value` as compact JSON and stream it to `path` through a
/// [`BufWriter`], skipping the intermediate `Vec<u8>` that
/// `serde_json::to_vec` + `std::fs::write` would allocate.
///
/// Synchronous on purpose: the caller in [`crate::util::manifest_store`] runs
/// it on a dedicated OS thread so manifest persistence never touches the
/// async runtime's worker or blocking pool. Async callers should wrap this
/// in `tokio::task::spawn_blocking` (or write the async-aware counterpart
/// when one is needed).
///
/// The parent directory of `path` must already exist; this helper does *not*
/// `mkdir -p`. The cost-benefit of "try the write first, recover on
/// `NotFound`" is policy-level (warm-cache rewrites want to skip the extra
/// syscall every time), so the recovery loop lives at the call site, not
/// here. A missing parent surfaces as [`io::ErrorKind::NotFound`] for the
/// caller to match on.
///
/// Serialization failures — rare for `derive(Serialize)` types, possible for
/// hand-written impls or maps with non-string keys — are folded into
/// [`io::Error`] via [`io::Error::other`] so the whole API speaks one error
/// type and callers can keep matching on [`io::ErrorKind`].
pub fn write_compact_sync<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, value).map_err(io::Error::other)?;
    writer.flush()
}

/// Parse package.json content into the caller's chosen view type `T`,
/// tolerating duplicate keys.
///
/// Some published npm packages ship package.json with duplicate keys
/// (e.g. `mime@1.3.4` has two `scripts` entries). serde's derived
/// `Deserialize` for structs rejects duplicate fields, but
/// `serde_json::Value` (backed by a Map) silently keeps the last value.
/// So when direct struct deserialization fails while the JSON itself is
/// valid, we retry through a Value intermediary to collapse duplicates.
/// The normal (no-duplicate) path has zero extra overhead.
///
/// `pkg_path` is used for error/log context only. This is the single
/// implementation of the lenient contract — both the async loader here and
/// the sync loader in `util::cloner` go through it so the tolerance can't
/// drift between paths.
pub fn parse_package_json_lenient<T: DeserializeOwned>(
    content: &str,
    pkg_path: &Path,
) -> Result<T> {
    match serde_json::from_str(content) {
        Ok(v) => Ok(v),
        Err(original_err) => match serde_json::from_str::<serde_json::Value>(content) {
            Ok(value) => {
                tracing::warn!(
                    "package.json has non-standard structure (e.g. duplicate keys), \
                     retrying with lenient parser: {pkg_path:?}"
                );
                serde_json::from_value(value)
                    .with_context(|| format!("Failed to deserialize {pkg_path:?}"))
            }
            Err(_) => {
                Err(original_err).with_context(|| format!("Failed to parse JSON from {pkg_path:?}"))
            }
        },
    }
}

/// Load package.json from a directory path and deserialize into the caller's
/// chosen view type `T`. Use a full `PackageJson` for root projects, or a
/// minimal view (e.g. `ScriptsView`) for node_modules to avoid parsing
/// unnecessary / non-standard fields.
pub async fn load_package_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let pkg_path = path.join("package.json");
    let content = crate::fs::read_to_string(&pkg_path)
        .await
        .with_context(|| format!("Failed to read file {pkg_path:?}"))?;
    parse_package_json_lenient(&content, &pkg_path)
}

pub async fn load_package_lock_json_from_path(
    path: &Path,
) -> Result<utoo_ruborist::lock::PackageLock> {
    read_json_file(&path.join("package-lock.json")).await
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use std::fs;
    use tempfile::tempdir;
    use utoo_ruborist::manifest::{PackageJson, ScriptsView};

    use super::*;

    #[tokio::test]
    async fn test_read_json_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.json");

        // Create a test JSON file
        let test_data = json!({
            "name": "test",
            "version": "1.0.0",
            "dependencies": {
                "dep1": "^1.0.0"
            }
        });

        fs::write(&file_path, test_data.to_string()).unwrap();

        // Test reading into Value
        let value: Value = read_json_file(&file_path).await.unwrap();
        assert_eq!(value["name"], "test");
        assert_eq!(value["version"], "1.0.0");

        // Test reading into custom type
        #[derive(serde::Deserialize)]
        struct TestPackage {
            name: String,
            version: String,
        }

        let package: TestPackage = read_json_file(&file_path).await.unwrap();
        assert_eq!(package.name, "test");
        assert_eq!(package.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_load_package_json() {
        let dir = tempdir().unwrap();
        let package_path = dir.path().join("package.json");

        let test_data = json!({
            "name": "test-package",
            "version": "1.0.0"
        });

        fs::write(&package_path, test_data.to_string()).unwrap();

        let pkg: PackageJson = load_package_json(dir.path()).await.unwrap();
        assert_eq!(pkg.name, "test-package");
        assert_eq!(pkg.version, "1.0.0");
    }

    /// Regression test: mime@1.3.4 has two "scripts" keys in its package.json.
    /// We must tolerate this and keep the last value (matching JSON.parse).
    #[tokio::test]
    async fn test_load_package_json_duplicate_fields() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{
                "name": "mime",
                "version": "1.3.4",
                "scripts": { "test": "node test.js" },
                "scripts": { "prepublish": "node build.js", "test": "node build/test.js" }
            }"#,
        )
        .unwrap();

        let view: ScriptsView = load_package_json(dir.path()).await.unwrap();
        // Last value wins, matching JSON.parse semantics
        assert_eq!(view.scripts.get("prepublish").unwrap(), "node build.js");
        assert_eq!(view.scripts.get("test").unwrap(), "node build/test.js");
    }

    #[tokio::test]
    async fn write_compact_sync_round_trips_through_read_json_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("out.json");
        let value = json!({
            "name": "test",
            "version": "1.0.0",
            "deps": ["a", "b", "c"],
        });

        super::write_compact_sync(&path, &value).unwrap();

        let read_back: Value = read_json_file(&path).await.unwrap();
        assert_eq!(read_back, value);

        // Compact form: no inter-token whitespace.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(": "));
        assert!(!raw.contains(", "));
    }

    #[test]
    fn write_compact_sync_requires_existing_parent_directory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing").join("out.json");
        let err = super::write_compact_sync(&path, &json!({})).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn test_error_handling() {
        let non_existent_path = Path::new("non_existent.json");

        // Test error handling for non-existent file
        let result: Result<Value> = read_json_file(non_existent_path).await;
        assert!(result.is_err());

        // Test error handling for invalid JSON
        let dir = tempdir().unwrap();
        let invalid_json_path = dir.path().join("invalid.json");
        fs::write(&invalid_json_path, "invalid json content").unwrap();

        let result: Result<Value> = read_json_file(&invalid_json_path).await;
        assert!(result.is_err());
    }
}
