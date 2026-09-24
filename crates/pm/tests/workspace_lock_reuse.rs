use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use tempfile::tempdir;

fn resolve_dependencies(project: &Path, cache: &Path, registry: &str) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_utoo"))
        .current_dir(project)
        .env("UTOO_CACHE_DIR", cache)
        .env("NO_UPDATE_NOTIFIER", "1")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .args(["deps", "--registry", registry])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&fs::read(project.join("package-lock.json")).unwrap()).unwrap()
}

#[test]
fn workspace_nested_dependency_preserves_locked_metadata() {
    let mut server = mockito::Server::new();
    let resolved = format!("{}/shared-2.0.0.tgz", server.url());
    // The registry now has more metadata than the original lockfile. Reusing
    // the workspace's pinned package must not fetch and replace its manifest.
    let manifest = json!({
        "name": "shared", "version": "2.0.0",
        "dist": { "tarball": resolved },
        "license": "MIT", "scripts": { "test": "node test.js" }
    });
    let packument = server
        .mock("GET", "/shared")
        .with_body(
            json!({
                "name": "shared", "dist-tags": { "latest": "2.0.0" },
                "versions": { "2.0.0": manifest }
            })
            .to_string(),
        )
        .expect(0)
        .create();
    let version = server
        .mock("GET", "/shared/2.0.0")
        .with_body(manifest.to_string())
        .expect(0)
        .create();
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let root = json!({
        "name": "root", "version": "1.0.0", "workspaces": ["packages/*"],
        "dependencies": { "shared": "1.0.0" }
    });
    let workspace = json!({
        "name": "app", "version": "1.0.0",
        "dependencies": { "shared": "2.0.0" }
    });
    fs::create_dir_all(project.path().join("packages/app")).unwrap();
    fs::write(project.path().join("package.json"), root.to_string()).unwrap();
    fs::write(
        project.path().join("packages/app/package.json"),
        workspace.to_string(),
    )
    .unwrap();
    let baseline = json!({
        "name": "root", "version": "1.0.0", "lockfileVersion": 3, "requires": true,
        "packages": {
            "": root,
            "packages/app": workspace,
            "node_modules/app": { "name": "app", "resolved": "packages/app", "link": true },
            "node_modules/shared": {
                "name": "shared", "version": "1.0.0",
                "resolved": format!("{}/shared-1.0.0.tgz", server.url())
            },
            "packages/app/node_modules/shared": {
                "name": "shared", "version": "2.0.0", "resolved": resolved
            }
        }
    });
    fs::write(
        project.path().join("package-lock.json"),
        baseline.to_string(),
    )
    .unwrap();

    let lock = resolve_dependencies(project.path(), cache.path(), &server.url());
    assert_eq!(lock["packages"], baseline["packages"]);
    packument.assert();
    version.assert();
}

#[test]
fn changed_conditional_overrides_replace_locked_workspace_dependency() {
    let mut server = mockito::Server::new();
    let mut versions = json!({});
    let mut version_mocks = Vec::new();
    for version in ["1.0.0", "2.0.0", "3.0.0", "4.0.0"] {
        let manifest = json!({
            "name": "shared", "version": version,
            "dist": { "tarball": format!("{}/shared-{version}.tgz", server.url()) }
        });
        version_mocks.push(
            server
                .mock("GET", format!("/shared/{version}").as_str())
                .with_body(manifest.to_string())
                .create(),
        );
        versions[version] = manifest;
    }
    let _packument = server
        .mock("GET", "/shared")
        .with_body(
            json!({
                "name": "shared", "dist-tags": { "latest": "4.0.0" }, "versions": versions
            })
            .to_string(),
        )
        .create();
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let manifest_path = project.path().join("package.json");
    let mut root = json!({
        "name": "root", "version": "1.0.0", "workspaces": ["packages/*"],
        "dependencies": { "shared": "1.0.0" }
    });
    fs::write(&manifest_path, root.to_string()).unwrap();
    fs::create_dir_all(project.path().join("packages/app")).unwrap();
    fs::write(
        project.path().join("packages/app/package.json"),
        json!({
            "name": "app", "version": "1.0.0",
            "dependencies": { "shared": "2.0.0" }
        })
        .to_string(),
    )
    .unwrap();

    let initial = resolve_dependencies(project.path(), cache.path(), &server.url());
    let nested = "packages/app/node_modules/shared";
    assert_eq!(initial["packages"][nested]["version"], "2.0.0");

    // Add and change overrides without deleting the workspace's existing lock.
    for (selector, target, expected) in [
        ("shared@^9.0.0", "3.0.0", "2.0.0"),
        ("shared@^2.0.0", "2.0.0", "2.0.0"),
        ("shared@^2.0.0", "latest", "4.0.0"),
        ("shared@^2.0.0", "3.0.0", "3.0.0"),
        ("shared@^2.0.0", "4.0.0", "4.0.0"),
    ] {
        root["overrides"] = json!({ "app": { (selector): target } });
        fs::write(&manifest_path, root.to_string()).unwrap();
        let warm = resolve_dependencies(project.path(), cache.path(), &server.url());
        assert_eq!(
            warm["packages"][nested]["version"], expected,
            "{selector} => {target}"
        );
        assert_eq!(
            warm["packages"]["node_modules/shared"],
            initial["packages"]["node_modules/shared"],
        );

        fs::remove_file(project.path().join("package-lock.json")).unwrap();
        let cold = resolve_dependencies(project.path(), cache.path(), &server.url());
        assert_eq!(
            warm, cold,
            "lockfile reuse differs for {selector} => {target}"
        );
    }
}

#[test]
fn unchanged_dist_tag_override_preserves_locked_version() {
    for (selector, requirement, first_version, next_version) in [
        ("shared", "latest", "1.0.0", "2.0.0"),
        ("shared@^1.0.0", "latest", "1.0.0", "2.0.0"),
        ("shared@^1.0.0", "1.0.0", "2.0.0", "3.0.0"),
    ] {
        let mut server = mockito::Server::new();
        let mut versions = json!({});
        let mut version_mocks = Vec::new();
        for version in ["1.0.0", "2.0.0", "3.0.0"] {
            let manifest = json!({
                "name": "shared", "version": version,
                "dist": { "tarball": format!("{}/shared-{version}.tgz", server.url()) }
            });
            version_mocks.push(
                server
                    .mock("GET", format!("/shared/{version}").as_str())
                    .with_body(manifest.to_string())
                    .create(),
            );
            versions[version] = manifest;
        }
        let mut packument = json!({
            "name": "shared", "dist-tags": { "latest": first_version, "next": next_version },
            "versions": versions
        });
        let initial_metadata = server
            .mock("GET", "/shared")
            .with_body(packument.to_string())
            .create();
        let project = tempdir().unwrap();
        let root = json!({
            "name": "root", "version": "1.0.0", "private": true,
            "dependencies": { "shared": requirement },
            "overrides": { (selector): "latest" }
        });
        fs::write(project.path().join("package.json"), root.to_string()).unwrap();
        let initial =
            resolve_dependencies(project.path(), tempdir().unwrap().path(), &server.url());
        assert_eq!(
            initial["packages"]["node_modules/shared"]["version"],
            first_version
        );
        initial_metadata.remove();
        for mock in &version_mocks {
            mock.remove();
        }

        packument["dist-tags"]["latest"] = json!(next_version);
        let moved_metadata = server
            .mock("GET", "/shared")
            .with_body(packument.to_string())
            .expect(0)
            .create();
        let version_metadata = server
            .mock("GET", mockito::Matcher::Regex("^/shared/".into()))
            .with_status(500)
            .expect(0)
            .create();
        // A fresh cache must not turn an unchanged lock into a tag update.
        let locked = resolve_dependencies(project.path(), tempdir().unwrap().path(), &server.url());
        assert_eq!(
            locked, initial,
            "unchanged {selector} override must retain its lock"
        );
        moved_metadata.assert();
        version_metadata.assert();
        moved_metadata.remove();
        version_metadata.remove();

        for (version, manifest) in versions.as_object().unwrap() {
            version_mocks.push(
                server
                    .mock("GET", format!("/shared/{version}").as_str())
                    .with_body(manifest.to_string())
                    .create(),
            );
        }
        // Either a changed requirement or a changed override must resolve again.
        for field in ["dependencies", "overrides"] {
            let changed_metadata = server
                .mock("GET", "/shared")
                .with_body(packument.to_string())
                .expect_at_least(1)
                .create();
            let mut changed_root = root.clone();
            changed_root[field] = json!({ "shared": "next" });
            fs::write(
                project.path().join("package.json"),
                changed_root.to_string(),
            )
            .unwrap();
            fs::write(
                project.path().join("package-lock.json"),
                initial.to_string(),
            )
            .unwrap();
            let changed =
                resolve_dependencies(project.path(), tempdir().unwrap().path(), &server.url());
            assert_eq!(
                changed["packages"]["node_modules/shared"]["version"], next_version,
                "changed {field}"
            );
            changed_metadata.assert();
            changed_metadata.remove();
        }
    }
}

#[test]
fn legacy_lock_preserves_pins_when_override_metadata_is_missing() {
    check_override_lock_pins(
        json!({ "unrelated@^9.0.0": "9.0.0" }),
        None,
        "1.0.0",
        "1.0.0",
    );
    check_override_lock_pins(
        json!({ "shared": "1.0.0", "unrelated@^9": { "inner": "9.0.0" } }),
        None,
        "1.0.0",
        "1.0.0",
    );
    check_override_lock_pins(json!({ "shared": "1.0.0" }), None, "1.0.0", "1.0.0");
    check_override_lock_pins(
        json!({ "shared": { "inner": "1.1.0" } }),
        None,
        "1.0.0",
        "1.1.0",
    );
}

#[test]
fn changed_nested_override_preserves_unaffected_pins() {
    check_override_lock_pins(
        json!({ "shared": { "inner": "1.1.0" } }),
        Some(json!({ "shared": { "inner": "1.0.0" } })),
        "1.0.0",
        "1.1.0",
    );
    check_override_lock_pins(
        json!({}),
        Some(json!({ "shared": { "inner": "1.1.0" } })),
        "1.1.0",
        "1.0.0",
    );
}

fn check_override_lock_pins(
    overrides: Value,
    previous: Option<Value>,
    locked_inner: &str,
    expected_inner: &str,
) {
    let mut server = mockito::Server::new();
    let mut mocks = Vec::new();
    let root = json!({
        "name": "root", "version": "1.0.0", "private": true,
        "dependencies": { "shared": "1.0.0", "unrelated": "^1.0.0" },
        "overrides": overrides
    });
    let mut locked_root = root.clone();
    locked_root.as_object_mut().unwrap().remove("overrides");
    locked_root.as_object_mut().unwrap().remove("private");
    if let Some(previous) = previous {
        locked_root["overrides"] = previous;
    }
    let mut packages = json!({ "": locked_root });
    for name in ["shared", "inner", "unrelated"] {
        let mut versions = json!({});
        for version in ["1.0.0", "1.1.0"] {
            let mut manifest = json!({
                "name": name, "version": version,
                "dist": { "tarball": format!("{}/{name}-{version}.tgz", server.url()) }
            });
            if name == "shared" {
                manifest["dependencies"] = json!({ "inner": "1.0.0" });
            }
            let mock = server
                .mock("GET", format!("/{name}/{version}").as_str())
                .with_body(manifest.to_string());
            mocks.push(
                if name == "unrelated" {
                    mock.expect(0)
                } else {
                    mock.expect_at_least(0)
                }
                .create(),
            );
            versions[version] = manifest;
        }
        let mock = server.mock("GET", format!("/{name}").as_str()).with_body(
            json!({
                "name": name, "dist-tags": { "latest": "1.1.0" }, "versions": versions
            })
            .to_string(),
        );
        mocks.push(
            if name == "unrelated" {
                mock.expect(0)
            } else {
                mock.expect_at_least(0)
            }
            .create(),
        );
        let version = if name == "inner" {
            locked_inner
        } else {
            "1.0.0"
        };
        packages[format!("node_modules/{name}")] = json!({
            "version": version, "resolved": format!("{}/{name}-{version}.tgz", server.url())
        });
    }
    packages["node_modules/shared"]["dependencies"] = json!({ "inner": "1.0.0" });
    let project = tempdir().unwrap();
    fs::write(project.path().join("package.json"), root.to_string()).unwrap();
    fs::write(
        project.path().join("package-lock.json"),
        json!({
            "name": "root", "version": "1.0.0", "lockfileVersion": 3, "requires": true,
            "packages": packages
        })
        .to_string(),
    )
    .unwrap();
    for _ in 0..2 {
        let lock = resolve_dependencies(project.path(), tempdir().unwrap().path(), &server.url());
        assert_eq!(
            lock["packages"]["node_modules/unrelated"]["version"],
            "1.0.0"
        );
        assert_eq!(lock["packages"]["node_modules/shared"]["version"], "1.0.0");
        let inner = lock["packages"]
            .get("node_modules/shared/node_modules/inner")
            .unwrap_or(&lock["packages"]["node_modules/inner"]);
        assert_eq!(inner["version"], expected_inner);
        assert_eq!(lock["packages"][""]["overrides"], root["overrides"]);
    }
    for mock in mocks {
        mock.assert();
    }
}
