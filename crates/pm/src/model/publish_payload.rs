use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use std::collections::HashMap;
use utoo_ruborist::manifest::PackageJson;

use crate::service::provenance::ProvenanceBundle;

/// Input for building the publish payload.
pub(crate) struct PublishPayloadInput<'a> {
    pub package_json: &'a PackageJson,
    pub name: &'a str,
    pub version: &'a str,
    pub tag: &'a str,
    pub shasum: &'a str,
    pub integrity: &'a str,
    pub tarball_data: &'a [u8],
    pub registry: &'a str,
    pub access: Option<&'a str>,
    /// Signed provenance bundle to attach alongside the tarball, if any.
    pub provenance: Option<&'a ProvenanceBundle>,
}

/// npm registry PUT payload for publishing a package.
#[derive(Serialize)]
pub(crate) struct PublishPayload {
    _id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    access: Option<String>,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    versions: HashMap<String, serde_json::Value>,
    _attachments: HashMap<String, Attachment>,
}

/// Attachment embedded in the publish payload (tarball or provenance bundle).
#[derive(Serialize)]
struct Attachment {
    content_type: String,
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
        let mut version_metadata = input.package_json.to_value();
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

        let description = input.package_json.description.clone();

        let mut attachments = HashMap::from([(
            tarball_name,
            Attachment {
                content_type: "application/octet-stream".to_string(),
                data: tarball_base64,
                length: input.tarball_data.len(),
            },
        )]);

        // npm attaches the provenance bundle as a second `_attachments` entry
        // named `<name>-<version>.sigstore`; the registry verifies it against
        // the tarball subject digest.
        if let Some(bundle) = input.provenance {
            attachments.insert(
                format!("{}-{}.sigstore", input.name, input.version),
                Attachment {
                    content_type: bundle.media_type.clone(),
                    length: bundle.data.len(),
                    data: bundle.data.clone(),
                },
            );
        }

        Self {
            _id: input.name.to_string(),
            name: input.name.to_string(),
            description,
            access: input.access.map(String::from),
            dist_tags: HashMap::from([(input.tag.to_string(), input.version.to_string())]),
            versions: HashMap::from([(input.version.to_string(), version_metadata)]),
            _attachments: attachments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_structure() {
        let pkg_json = PackageJson {
            name: "test-pkg".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test package".to_string()),
            ..Default::default()
        };

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
            provenance: None,
        });

        assert_eq!(payload._id, "test-pkg");
        assert_eq!(payload.name, "test-pkg");
        assert_eq!(payload.description.as_deref(), Some("A test package"));
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
        let pkg_json = PackageJson::new("@eggjs/tegg-plugin", "3.72.0");

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
            provenance: None,
        });

        // description is None when not in package.json
        assert!(payload.description.is_none());

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
            payload
                ._attachments
                .contains_key("@eggjs/tegg-plugin-3.72.0.tgz"),
        );
    }

    #[test]
    fn test_payload_custom_tag() {
        let pkg_json = PackageJson::new("pkg", "2.0.0-rc.1");
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
            provenance: None,
        });

        assert_eq!(payload.dist_tags["beta"], "2.0.0-rc.1");
        assert!(!payload.dist_tags.contains_key("latest"));
    }

    #[test]
    fn test_payload_provenance_attachment() {
        let pkg_json = PackageJson::new("@scope/pkg", "1.0.0");
        let bundle = ProvenanceBundle {
            media_type: "application/vnd.dev.sigstore.bundle.v0.3+json".to_string(),
            data: r#"{"mediaType":"x"}"#.to_string(),
        };

        let payload = PublishPayload::new(&PublishPayloadInput {
            package_json: &pkg_json,
            name: "@scope/pkg",
            version: "1.0.0",
            tag: "latest",
            shasum: "s",
            integrity: "i",
            tarball_data: b"data",
            registry: "https://registry.npmjs.org",
            access: Some("public"),
            provenance: Some(&bundle),
        });

        // The tarball and the `.sigstore` bundle are both attached.
        assert!(payload._attachments.contains_key("@scope/pkg-1.0.0.tgz"));
        let att = &payload._attachments["@scope/pkg-1.0.0.sigstore"];
        assert_eq!(
            att.content_type,
            "application/vnd.dev.sigstore.bundle.v0.3+json"
        );
        assert_eq!(att.length, bundle.data.len());
        assert_eq!(att.data, bundle.data);
    }

    #[test]
    fn test_payload_without_provenance_has_no_sigstore_attachment() {
        let pkg_json = PackageJson::new("pkg", "1.0.0");
        let payload = PublishPayload::new(&PublishPayloadInput {
            package_json: &pkg_json,
            name: "pkg",
            version: "1.0.0",
            tag: "latest",
            shasum: "s",
            integrity: "i",
            tarball_data: b"d",
            registry: "https://r.test",
            access: None,
            provenance: None,
        });
        assert_eq!(payload._attachments.len(), 1);
        assert!(payload._attachments.contains_key("pkg-1.0.0.tgz"));
    }
}
