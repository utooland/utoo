use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use std::collections::HashMap;

/// Input for building the publish payload.
pub(crate) struct PublishPayloadInput<'a> {
    pub package_json: &'a serde_json::Value,
    pub name: &'a str,
    pub version: &'a str,
    pub tag: &'a str,
    pub shasum: &'a str,
    pub integrity: &'a str,
    pub tarball_data: &'a [u8],
    pub tarball_filename: &'a str,
    pub registry: &'a str,
}

/// npm registry PUT payload for publishing a package.
#[derive(Serialize)]
pub(crate) struct PublishPayload {
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

impl PublishPayload {
    /// Build the publish PUT payload from the given input.
    pub fn new(input: &PublishPayloadInput<'_>) -> Self {
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

        Self {
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

    /// Build the tarball filename from package name and version.
    ///
    /// Scoped packages have `@` and `/` stripped, e.g. `@scope/pkg` → `scope-pkg-1.0.0.tgz`.
    pub fn tarball_filename(name: &str, version: &str) -> String {
        format!(
            "{}-{}.tgz",
            name.replace('/', "-").replace('@', ""),
            version
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tarball_filename_simple() {
        assert_eq!(
            PublishPayload::tarball_filename("my-pkg", "1.0.0"),
            "my-pkg-1.0.0.tgz"
        );
    }

    #[test]
    fn test_tarball_filename_scoped() {
        assert_eq!(
            PublishPayload::tarball_filename("@scope/my-pkg", "2.3.4"),
            "scope-my-pkg-2.3.4.tgz"
        );
    }

    #[test]
    fn test_tarball_filename_prerelease() {
        assert_eq!(
            PublishPayload::tarball_filename("pkg", "1.0.0-beta.1"),
            "pkg-1.0.0-beta.1.tgz"
        );
    }

    #[test]
    fn test_payload_structure() {
        let pkg_json = serde_json::json!({
            "name": "test-pkg",
            "version": "1.0.0"
        });

        let payload = PublishPayload::new(&PublishPayloadInput {
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
    fn test_payload_custom_tag() {
        let pkg_json = serde_json::json!({ "name": "pkg", "version": "2.0.0-rc.1" });
        let payload = PublishPayload::new(&PublishPayloadInput {
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
