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
    pub registry: &'a str,
    pub access: Option<&'a str>,
}

/// npm registry PUT payload for publishing a package.
#[derive(Serialize)]
pub(crate) struct PublishPayload {
    _id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    access: Option<String>,
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
    ///
    /// The tarball name and URL are derived from `name` and `version` following
    /// the npm registry protocol (`libnpmpublish/lib/publish.js`):
    /// ```js
    /// const tarballName = `${manifest.name}-${manifest.version}.tgz`
    /// const tarballURI  = `${manifest.name}/-/${tarballName}`
    /// ```
    pub fn new(input: &PublishPayloadInput<'_>) -> Self {
        let tarball_base64 = BASE64.encode(input.tarball_data);

        // npm: `${name}-${version}.tgz` — preserves scope for scoped packages.
        let tarball_name = format!("{}-{}.tgz", input.name, input.version);

        // Inject dist and _id into version metadata
        let mut version_metadata = input.package_json.clone();
        if let Some(obj) = version_metadata.as_object_mut() {
            obj.insert(
                "dist".to_string(),
                serde_json::to_value(Dist {
                    shasum: input.shasum.to_string(),
                    integrity: input.integrity.to_string(),
                    // npm: `${name}/-/${tarballName}` resolved against registry
                    tarball: format!(
                        "{}/{}/-/{}",
                        input.registry.trim_end_matches('/'),
                        input.name,
                        tarball_name,
                    ),
                })
                .expect("Dist serialization cannot fail"),
            );
            obj.insert(
                "_id".to_string(),
                format!("{}@{}", input.name, input.version).into(),
            );
        }

        Self {
            _id: input.name.to_string(),
            name: input.name.to_string(),
            access: input.access.map(String::from),
            dist_tags: HashMap::from([(input.tag.to_string(), input.version.to_string())]),
            versions: HashMap::from([(input.version.to_string(), version_metadata)]),
            _attachments: HashMap::from([(
                tarball_name,
                Attachment {
                    content_type: "application/octet-stream",
                    data: tarball_base64,
                    length: input.tarball_data.len(),
                },
            )]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            registry: "https://registry.npmjs.org",
            access: None,
        });

        assert_eq!(payload._id, "test-pkg");
        assert_eq!(payload.name, "test-pkg");
        assert_eq!(payload.dist_tags["latest"], "1.0.0");

        let ver = &payload.versions["1.0.0"];
        assert_eq!(ver["name"], "test-pkg");
        assert_eq!(ver["_id"], "test-pkg@1.0.0");
        assert_eq!(ver["dist"]["shasum"], "abc123shasum");
        assert_eq!(ver["dist"]["integrity"], "sha512-integrity");
        assert_eq!(
            ver["dist"]["tarball"].as_str().unwrap(),
            "https://registry.npmjs.org/test-pkg/-/test-pkg-1.0.0.tgz",
        );

        let att = &payload._attachments["test-pkg-1.0.0.tgz"];
        assert_eq!(att.content_type, "application/octet-stream");
        assert_eq!(att.length, b"fake-tarball".len());

        let decoded = BASE64.decode(&att.data).unwrap();
        assert_eq!(decoded, b"fake-tarball");
    }

    /// Verify scoped package payload matches npm registry protocol.
    ///
    /// npm source (`libnpmpublish/lib/publish.js`):
    /// ```js
    /// const tarballName = `${manifest.name}-${manifest.version}.tgz`
    /// const tarballURI  = `${manifest.name}/-/${tarballName}`
    /// manifest.dist.tarball = new URL(tarballURI, registry).href
    /// root._attachments[tarballName] = { ... }
    /// ```
    ///
    /// For `@eggjs/tegg-plugin@3.72.0`:
    ///   - PUT tarballName  = `@eggjs/tegg-plugin-3.72.0.tgz`
    ///   - PUT tarball URL  = `https://registry.npmjs.org/@eggjs/tegg-plugin/-/@eggjs/tegg-plugin-3.72.0.tgz`
    ///   - GET tarball URL  = `https://registry.npmjs.org/@eggjs/tegg-plugin/-/tegg-plugin-3.72.0.tgz`
    ///     (registry rewrites the stored URL, stripping the scope from filename)
    #[test]
    fn test_payload_scoped_package() {
        let pkg_json = serde_json::json!({
            "name": "@eggjs/tegg-plugin",
            "version": "3.72.0"
        });

        let payload = PublishPayload::new(&PublishPayloadInput {
            package_json: &pkg_json,
            name: "@eggjs/tegg-plugin",
            version: "3.72.0",
            tag: "latest",
            shasum: "abc",
            integrity: "sha512-xyz",
            tarball_data: b"data",
            registry: "https://registry.npmjs.org",
            access: Some("public"),
        });

        // dist.tarball: {registry}/{name}/-/{name}-{version}.tgz
        let tarball_url = payload.versions["3.72.0"]["dist"]["tarball"]
            .as_str()
            .unwrap();
        assert_eq!(
            tarball_url,
            "https://registry.npmjs.org/@eggjs/tegg-plugin/-/@eggjs/tegg-plugin-3.72.0.tgz",
        );

        // _attachments key: {name}-{version}.tgz (full scoped name)
        assert!(
            payload._attachments.contains_key("@eggjs/tegg-plugin-3.72.0.tgz"),
        );
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
            registry: "https://r.test",
            access: None,
        });

        assert_eq!(payload.dist_tags["beta"], "2.0.0-rc.1");
        assert!(!payload.dist_tags.contains_key("latest"));
    }
}
