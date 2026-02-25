use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use std::collections::HashMap;

use crate::model::package::PackageInfo;
use crate::service::auth;
use crate::service::pm_pack;
use crate::service::script::ScriptService;
use crate::util::integrity::compute_shasum;
use crate::util::json::load_package_json_from_path;

/// npm registry PUT payload for publishing a package.
#[derive(Serialize)]
struct PublishPayload {
    _id: String,
    name: String,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    versions: HashMap<String, serde_json::Value>,
    _attachments: HashMap<String, Attachment>,
}

/// Tarball attachment embedded in the publish payload.
#[derive(Serialize)]
struct Attachment {
    content_type: &'static str,
    data: String,
    length: usize,
}

/// Dist metadata injected into the version entry.
#[derive(Serialize)]
struct Dist {
    shasum: String,
    integrity: String,
    tarball: String,
}

/// Result returned to the cmd layer after a successful publish.
pub struct PublishResult {
    pub name: String,
    pub version: String,
    pub tag: String,
    pub registry: String,
}

pub async fn publish(
    package_info: &PackageInfo,
    registry: &str,
    tag: &str,
    dry_run: bool,
    otp: Option<&str>,
) -> Result<PublishResult> {
    // Resolve auth token against the publish registry
    let token = auth::require_token(registry).await?;

    // Run prepublishOnly lifecycle script
    ScriptService::execute_script(package_info, "prepublishOnly", true).await?;

    // Pack the package (prepack/postpack scripts run inside pack)
    let pack_result = pm_pack::pack(&package_info.path, false).await?;

    let tarball_path = pack_result
        .tarball_path
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No tarball created"))?;

    // Read tarball data
    let tarball_data = tokio::fs::read(tarball_path)
        .await
        .context("Failed to read tarball")?;

    let shasum = compute_shasum(&tarball_data);

    let result = PublishResult {
        name: pack_result.name.clone(),
        version: pack_result.version.clone(),
        tag: tag.to_string(),
        registry: registry.to_string(),
    };

    if dry_run {
        let _ = tokio::fs::remove_file(tarball_path).await;
        return Ok(result);
    }

    // Load package.json for version metadata in the publish payload
    let package_json = load_package_json_from_path(&package_info.path).await?;
    let tarball_filename = build_tarball_filename(&pack_result.name, &pack_result.version);
    let payload = build_publish_payload(&PayloadInput {
        package_json: &package_json,
        name: &pack_result.name,
        version: &pack_result.version,
        tag,
        shasum: &shasum,
        integrity: &pack_result.integrity,
        tarball_data: &tarball_data,
        tarball_filename: &tarball_filename,
        registry,
    });

    // Send PUT request to registry
    let url = format!("{}/{}", registry.trim_end_matches('/'), pack_result.name);

    let mut req = crate::util::http::client()
        .put(&url)
        .header("content-type", "application/json")
        .bearer_auth(&token)
        .json(&payload);

    if let Some(otp) = otp {
        req = req.header("npm-otp", otp);
    }

    let response = req.send().await.context("Failed to send publish request")?;
    let status = response.status();

    // Clean up tarball
    let _ = tokio::fs::remove_file(tarball_path).await;

    match status.as_u16() {
        200 | 201 => {}
        401 => {
            let body = response.text().await.unwrap_or_default();
            if body.contains("EOTP") || body.contains("one-time pass") {
                return Err(anyhow::anyhow!(
                    "This operation requires a one-time password.\n\
                     Use `utoo publish --otp <code>` to provide one."
                ));
            }
            return Err(anyhow::anyhow!(
                "Authentication failed. Check your credentials or run `utoo login`.\n{body}"
            ));
        }
        403 => {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Forbidden: {}", body));
        }
        409 => {
            return Err(anyhow::anyhow!(
                "{}@{} already exists. Use a different version.",
                pack_result.name,
                pack_result.version,
            ));
        }
        other => {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Publish failed (HTTP {}): {}", other, body));
        }
    }

    // Run publish lifecycle scripts
    ScriptService::execute_script(package_info, "publish", true).await?;
    ScriptService::execute_script(package_info, "postpublish", true).await?;

    Ok(result)
}

/// Build the tarball filename from package name and version.
///
/// Scoped packages have `@` and `/` stripped, e.g. `@scope/pkg` → `scope-pkg-1.0.0.tgz`.
fn build_tarball_filename(name: &str, version: &str) -> String {
    format!(
        "{}-{}.tgz",
        name.replace('/', "-").replace('@', ""),
        version
    )
}

/// Input for building the publish payload.
struct PayloadInput<'a> {
    package_json: &'a serde_json::Value,
    name: &'a str,
    version: &'a str,
    tag: &'a str,
    shasum: &'a str,
    integrity: &'a str,
    tarball_data: &'a [u8],
    tarball_filename: &'a str,
    registry: &'a str,
}

/// Build the npm publish PUT payload.
fn build_publish_payload(input: &PayloadInput<'_>) -> PublishPayload {
    let tarball_base64 = BASE64.encode(input.tarball_data);

    // Inject dist and _id into version metadata
    let mut version_metadata = input.package_json.clone();
    if let Some(obj) = version_metadata.as_object_mut() {
        obj.insert(
            "dist".to_string(),
            serde_json::to_value(Dist {
                shasum: input.shasum.to_string(),
                integrity: input.integrity.to_string(),
                tarball: format!(
                    "{}/{}-/{}",
                    input.registry, input.name, input.tarball_filename
                ),
            })
            .unwrap(),
        );
        obj.insert(
            "_id".to_string(),
            format!("{}@{}", input.name, input.version).into(),
        );
    }

    PublishPayload {
        _id: input.name.to_string(),
        name: input.name.to_string(),
        dist_tags: HashMap::from([(input.tag.to_string(), input.version.to_string())]),
        versions: HashMap::from([(input.version.to_string(), version_metadata)]),
        _attachments: HashMap::from([(
            input.tarball_filename.to_string(),
            Attachment {
                content_type: "application/octet-stream",
                data: tarball_base64,
                length: input.tarball_data.len(),
            },
        )]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_tarball_filename_simple() {
        assert_eq!(
            build_tarball_filename("my-pkg", "1.0.0"),
            "my-pkg-1.0.0.tgz"
        );
    }

    #[test]
    fn test_build_tarball_filename_scoped() {
        assert_eq!(
            build_tarball_filename("@scope/my-pkg", "2.3.4"),
            "scope-my-pkg-2.3.4.tgz"
        );
    }

    #[test]
    fn test_build_tarball_filename_prerelease() {
        assert_eq!(
            build_tarball_filename("pkg", "1.0.0-beta.1"),
            "pkg-1.0.0-beta.1.tgz"
        );
    }

    #[test]
    fn test_build_publish_payload_structure() {
        let pkg_json = serde_json::json!({
            "name": "test-pkg",
            "version": "1.0.0"
        });

        let payload = build_publish_payload(&PayloadInput {
            package_json: &pkg_json,
            name: "test-pkg",
            version: "1.0.0",
            tag: "latest",
            shasum: "abc123shasum",
            integrity: "sha512-integrity",
            tarball_data: b"fake-tarball",
            tarball_filename: "test-pkg-1.0.0.tgz",
            registry: "https://registry.npmjs.org",
        });

        assert_eq!(payload._id, "test-pkg");
        assert_eq!(payload.name, "test-pkg");
        assert_eq!(payload.dist_tags["latest"], "1.0.0");

        let ver = &payload.versions["1.0.0"];
        assert_eq!(ver["name"], "test-pkg");
        assert_eq!(ver["_id"], "test-pkg@1.0.0");
        assert_eq!(ver["dist"]["shasum"], "abc123shasum");
        assert_eq!(ver["dist"]["integrity"], "sha512-integrity");
        assert!(
            ver["dist"]["tarball"]
                .as_str()
                .unwrap()
                .contains("test-pkg-1.0.0.tgz")
        );

        let att = &payload._attachments["test-pkg-1.0.0.tgz"];
        assert_eq!(att.content_type, "application/octet-stream");
        assert_eq!(att.length, b"fake-tarball".len());

        let decoded = BASE64.decode(&att.data).unwrap();
        assert_eq!(decoded, b"fake-tarball");
    }

    #[test]
    fn test_build_publish_payload_custom_tag() {
        let pkg_json = serde_json::json!({ "name": "pkg", "version": "2.0.0-rc.1" });
        let payload = build_publish_payload(&PayloadInput {
            package_json: &pkg_json,
            name: "pkg",
            version: "2.0.0-rc.1",
            tag: "beta",
            shasum: "s",
            integrity: "i",
            tarball_data: b"d",
            tarball_filename: "pkg-2.0.0-rc.1.tgz",
            registry: "https://r.test",
        });

        assert_eq!(payload.dist_tags["beta"], "2.0.0-rc.1");
        assert!(!payload.dist_tags.contains_key("latest"));
    }
}
