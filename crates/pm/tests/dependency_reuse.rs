use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use tempfile::tempdir;

fn tarball(manifest: Value, source: &str) -> Vec<u8> {
    let gzip = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(gzip);
    for (path, body) in [
        ("package/package.json", manifest.to_string()),
        ("package/index.js", source.to_string()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, path, body.as_bytes())
            .unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

fn check_tarball_deduplication(
    dependency_tarball: &str,
    override_rule: Option<(&str, &str)>,
    expected_tarball: &str,
) {
    let mut server = mockito::Server::new();
    let shared_url = format!("{}/shared.tgz", server.url());
    let dependency_url = format!("{}/{dependency_tarball}", server.url());
    let expected_url = format!("{}/{expected_tarball}", server.url());
    let same_url = expected_url == shared_url;
    let shared = tarball(
        json!({ "name": "shared", "version": "1.0.0", "main": "index.js" }),
        "module.exports = new Map();",
    );
    let _shared = server
        .mock("GET", "/shared.tgz")
        .with_body(&shared)
        .create();
    let _other = server.mock("GET", "/other.tgz").with_body(&shared).create();
    let _patched = server
        .mock("GET", "/patched.tgz")
        .with_body(tarball(
            json!({ "name": "shared", "version": "1.0.0", "main": "index.js" }),
            "module.exports = new Map([['patched', true]]);",
        ))
        .create();
    let _consumer = server
        .mock("GET", "/consumer.tgz")
        .with_body(tarball(
            json!({
                "name": "consumer", "version": "1.0.0", "main": "index.js",
                "dependencies": { "shared": dependency_url }
            }),
            "module.exports = require('shared');",
        ))
        .create();
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let mut manifest = json!({
        "name": "http-dedup", "version": "1.0.0", "private": true,
        "dependencies": {
            "shared": shared_url,
            "consumer": format!("{}/consumer.tgz", server.url())
        }
    });
    if let Some((spec, target)) = override_rule {
        manifest["overrides"] = json!({
            "consumer": { (spec): format!("{}/{target}", server.url()) }
        });
    }
    fs::write(project.path().join("package.json"), manifest.to_string()).unwrap();

    // Check both fresh resolution and installation from the generated lockfile.
    for reinstall in [false, true] {
        if reinstall {
            fs::remove_dir_all(project.path().join("node_modules")).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_utoo"))
            .current_dir(project.path())
            .env("UTOO_CACHE_DIR", cache.path())
            .env("NO_UPDATE_NOTIFIER", "1")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .args(["install", "--ignore-scripts"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");

        let lock: Value =
            serde_json::from_slice(&fs::read(project.path().join("package-lock.json")).unwrap())
                .unwrap();
        let packages = lock["packages"].as_object().unwrap();
        assert_eq!(packages.len(), if same_url { 3 } else { 4 });
        assert_eq!(packages["node_modules/shared"]["resolved"], shared_url);
        let nested = "node_modules/consumer/node_modules/shared";
        assert_eq!(packages.contains_key(nested), !same_url);
        assert_eq!(project.path().join(nested).exists(), !same_url);
        if !same_url {
            assert_eq!(packages[nested]["resolved"], expected_url);
        }

        let output = Command::new("node")
            .current_dir(project.path())
            .args([
                "-e",
                r#"
                const assert = require("node:assert/strict");
                const shared = require("shared");
                const consumer = require("consumer");
                const expectShared = process.argv[1] === "true";
                const expectPatched = process.argv[2] === "true";
                shared.set("example", 42);
                assert.equal(consumer === shared, expectShared);
                assert.equal(consumer.get("example"), expectShared ? 42 : undefined);
                assert.equal(consumer.get("patched"), expectPatched ? true : undefined);
                assert.equal(shared.get("patched"), undefined);
                "#,
            ])
            .arg(same_url.to_string())
            .arg((expected_tarball == "patched.tgz").to_string())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[test]
fn identical_http_tarballs_share_module_state() {
    check_tarball_deduplication("shared.tgz", None, "shared.tgz");
}

#[test]
fn different_http_tarballs_with_same_version_stay_separate() {
    check_tarball_deduplication("other.tgz", None, "other.tgz");
}

#[test]
fn conditional_override_prevents_http_tarball_reuse() {
    check_tarball_deduplication(
        "shared.tgz",
        Some(("shared@^1.0.0", "patched.tgz")),
        "patched.tgz",
    );
}

#[test]
fn nonmatching_conditional_override_allows_http_tarball_reuse() {
    check_tarball_deduplication(
        "shared.tgz",
        Some(("shared@^2.0.0", "patched.tgz")),
        "shared.tgz",
    );
}

#[test]
fn conditional_override_to_same_url_allows_http_tarball_reuse() {
    check_tarball_deduplication(
        "shared.tgz",
        Some(("shared@^1.0.0", "shared.tgz")),
        "shared.tgz",
    );
}

#[test]
fn unconditional_override_prevents_http_tarball_reuse() {
    check_tarball_deduplication("shared.tgz", Some(("shared", "patched.tgz")), "patched.tgz");
}

fn registry_package(
    server: &mut mockito::Server,
    name: &str,
    dependencies: Value,
    source: &str,
) -> Vec<mockito::Mock> {
    let manifest = json!({
        "name": name, "version": "1.0.0", "main": "index.js",
        "dependencies": dependencies,
        "dist": { "tarball": format!("{}/{name}.tgz", server.url()) }
    });
    vec![
        server
            .mock("GET", format!("/{name}").as_str())
            .with_body(
                json!({
                    "name": name, "dist-tags": { "latest": "1.0.0" },
                    "versions": { "1.0.0": manifest }
                })
                .to_string(),
            )
            .create(),
        server
            .mock("GET", format!("/{name}/1.0.0").as_str())
            .with_body(manifest.to_string())
            .create(),
        server
            .mock("GET", format!("/{name}.tgz").as_str())
            .with_body(tarball(manifest, source))
            .create(),
    ]
}

async fn run_utoo(project: &Path, cache: &Path, registry: &str, args: &[&str]) -> Value {
    // A reuse regression can expand a dependency cycle forever. Kill the child
    // on timeout so a failing test cannot hang the test runner or leave it running.
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_utoo"))
            .current_dir(project)
            .env("UTOO_CACHE_DIR", cache)
            .env("NO_UPDATE_NOTIFIER", "1")
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .args(args)
            .args(["--registry", registry])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("dependency resolution did not terminate")
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&fs::read(project.join("package-lock.json")).unwrap()).unwrap()
}

async fn check_conditional_override_shares_module_state(target: &str, http_dependency: bool) {
    let mut server = mockito::Server::new_async().await;
    let shared_spec = if http_dependency {
        format!("{}/shared.tgz", server.url())
    } else {
        "^1.0.0".to_string()
    };
    let _shared = registry_package(
        &mut server,
        "shared",
        json!({}),
        "module.exports = new Map();",
    );
    let _first = registry_package(
        &mut server,
        "first",
        json!({ "shared": shared_spec }),
        "module.exports = require('shared');",
    );
    let _second = registry_package(
        &mut server,
        "second",
        json!({ "shared": shared_spec }),
        "module.exports = require('shared');",
    );
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        json!({
            "name": "root", "version": "1.0.0", "private": true,
            "dependencies": { "first": "1.0.0", "second": "1.0.0" },
            "overrides": { "shared@^1.0.0": target }
        })
        .to_string(),
    )
    .unwrap();

    for reinstall in [false, true] {
        if reinstall {
            fs::remove_dir_all(project.path().join("node_modules")).unwrap();
        }
        let lock = run_utoo(
            project.path(),
            cache.path(),
            &server.url(),
            &["install", "--ignore-scripts"],
        )
        .await;
        assert_eq!(lock["packages"].as_object().unwrap().len(), 4);
        assert_eq!(lock["packages"]["node_modules/shared"]["version"], "1.0.0");

        let output = Command::new("node")
            .current_dir(project.path())
            .args([
                "-e",
                r#"
                const assert = require("node:assert/strict");
                const first = require("first");
                const second = require("second");
                first.set("example", 42);
                assert.equal(first, second);
                assert.equal(second.get("example"), 42);
            "#,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[tokio::test]
async fn compatible_conditional_override_shares_module_state() {
    check_conditional_override_shares_module_state("1.0.0", false).await;
}

#[tokio::test]
async fn conditional_dist_tag_override_shares_module_state() {
    check_conditional_override_shares_module_state("latest", false).await;
}

#[tokio::test]
async fn conditional_dist_tag_override_shares_http_dependency() {
    check_conditional_override_shares_module_state("latest", true).await;
}

async fn check_conditional_overrides_close_dependency_cycles(target: &str) {
    let mut server = mockito::Server::new_async().await;
    let _a = registry_package(&mut server, "cycle-a", json!({ "cycle-b": "^1.0.0" }), "");
    let _b = registry_package(&mut server, "cycle-b", json!({ "cycle-a": "^1.0.0" }), "");
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        json!({
            "name": "root", "version": "1.0.0", "private": true,
            "dependencies": { "cycle-a": target },
            "overrides": { "cycle-a@^1.0.0": target, "cycle-b@^1.0.0": target }
        })
        .to_string(),
    )
    .unwrap();

    // Resolve both with and without an existing lockfile.
    for _ in 0..2 {
        let lock = run_utoo(project.path(), cache.path(), &server.url(), &["deps"]).await;
        let packages = lock["packages"].as_object().unwrap();
        assert_eq!(packages.len(), 3);
        for (name, dependency) in [("cycle-a", "cycle-b"), ("cycle-b", "cycle-a")] {
            let package = &packages[&format!("node_modules/{name}")];
            assert_eq!(package["version"], "1.0.0");
            assert_eq!(package["dependencies"][dependency], "^1.0.0");
        }
    }
}

#[tokio::test]
async fn compatible_conditional_overrides_close_dependency_cycles() {
    check_conditional_overrides_close_dependency_cycles("1.0.0").await;
}

#[tokio::test]
async fn conditional_dist_tag_overrides_close_dependency_cycles() {
    check_conditional_overrides_close_dependency_cycles("latest").await;
}

#[tokio::test]
async fn http_reuse_preserves_descendant_overrides() {
    check_http_descendant_overrides(false).await;
    check_http_descendant_overrides(true).await;
}

async fn check_http_descendant_overrides(deep: bool) {
    let mut server = mockito::Server::new_async().await;
    let dependency = if deep { "bridge" } else { "inner" };
    let _bridge = deep.then(|| {
        registry_package(
            &mut server,
            "bridge",
            json!({ "inner": "1.0.0" }),
            "module.exports = require('inner');",
        )
    });
    let shared_url = format!("{}/shared.tgz", server.url());
    let _shared = server
        .mock("GET", "/shared.tgz")
        .with_body(tarball(
            json!({
                "name": "shared", "version": "1.0.0", "main": "index.js",
                "dependencies": { (dependency): "1.0.0" }
            }),
            &format!("module.exports = require('{dependency}');"),
        ))
        .create();
    let _consumer = server
        .mock("GET", "/consumer.tgz")
        .with_body(tarball(
            json!({
                "name": "consumer", "version": "1.0.0", "main": "index.js",
                "dependencies": { "shared": shared_url }
            }),
            "module.exports = require('shared');",
        ))
        .create();
    let mut versions = json!({});
    let mut inner_mocks = Vec::new();
    for version in ["1.0.0", "2.0.0"] {
        let manifest = json!({
            "name": "inner", "version": version, "main": "index.js",
            "dist": { "tarball": format!("{}/inner-{version}.tgz", server.url()) }
        });
        inner_mocks.push(
            server
                .mock("GET", format!("/inner/{version}").as_str())
                .with_body(manifest.to_string())
                .create(),
        );
        inner_mocks.push(
            server
                .mock("GET", format!("/inner-{version}.tgz").as_str())
                .with_body(tarball(
                    manifest.clone(),
                    &format!("module.exports = '{version}';"),
                ))
                .create(),
        );
        versions[version] = manifest;
    }
    let _inner = server
        .mock("GET", "/inner")
        .with_body(
            json!({
                "name": "inner", "dist-tags": { "latest": "2.0.0" }, "versions": versions
            })
            .to_string(),
        )
        .create();
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    let override_rule = if deep {
        json!({ "bridge": { "inner": "2.0.0" } })
    } else {
        json!({ "inner": "2.0.0" })
    };
    fs::write(
        project.path().join("package.json"),
        json!({
            "name": "root", "version": "1.0.0", "private": true,
            "dependencies": {
                "shared": shared_url,
                "consumer": format!("{}/consumer.tgz", server.url())
            },
            "overrides": { "consumer": { "shared": override_rule } }
        })
        .to_string(),
    )
    .unwrap();

    for reinstall in [false, true] {
        if reinstall {
            fs::remove_dir_all(project.path().join("node_modules")).unwrap();
        }
        run_utoo(
            project.path(),
            cache.path(),
            &server.url(),
            &["install", "--ignore-scripts"],
        )
        .await;
        let output = Command::new("node")
            .current_dir(project.path())
            .args([
                "-e",
                r#"
                const assert = require("node:assert/strict");
                assert.equal(require("consumer"), "2.0.0");
                assert.equal(require("shared"), "1.0.0");
            "#,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[tokio::test]
async fn equivalent_nested_overrides_preserve_module_identity() {
    for (override_rule, inner, cycle) in [
        (json!({ "inner": "1.0.0" }), "inner", false),
        (json!({ "unused": "2.0.0" }), "inner", false),
        (json!({ "z-inner": "1.0.0" }), "z-inner", false),
        (json!({ "inner": "latest" }), "inner", false),
        (json!({ "unused": "2.0.0" }), "z-inner", true),
    ] {
        let mut server = mockito::Server::new_async().await;
        let inner_dependencies = if cycle {
            json!({ "shared": "1.0.0" })
        } else {
            json!({})
        };
        let _inner = registry_package(&mut server, inner, inner_dependencies, "");
        let _shared = registry_package(
            &mut server,
            "shared",
            json!({ (inner): "1.0.0" }),
            "module.exports = new Map();",
        );
        let _consumer = registry_package(
            &mut server,
            "consumer",
            json!({ "shared": "1.0.0" }),
            "module.exports = require('shared');",
        );
        let project = tempdir().unwrap();
        let cache = tempdir().unwrap();
        fs::write(
            project.path().join("package.json"),
            json!({
                "name": "root", "version": "1.0.0", "private": true,
                "dependencies": { "shared": "1.0.0", "consumer": "1.0.0" },
                "overrides": { "consumer": { "shared": override_rule } }
            })
            .to_string(),
        )
        .unwrap();
        for reinstall in [false, true] {
            if reinstall {
                fs::remove_dir_all(project.path().join("node_modules")).unwrap();
            }
            let lock = run_utoo(
                project.path(),
                cache.path(),
                &server.url(),
                &["install", "--ignore-scripts"],
            )
            .await;
            let output = Command::new("node")
                .current_dir(project.path())
                .args([
                    "-e",
                    r#"
                const assert = require("node:assert/strict");
                const shared = require("shared");
                const consumer = require("consumer");
                shared.set("example", 42);
                assert.equal(shared, consumer);
                assert.equal(consumer.get("example"), 42);
            "#,
                ])
                .output()
                .unwrap();
            assert!(output.status.success(), "{override_rule}: {output:?}");
            assert_eq!(lock["packages"].as_object().unwrap().len(), 4);
        }
    }
}

#[tokio::test]
async fn changed_override_keeps_consumers_on_one_module_instance() {
    let mut server = mockito::Server::new_async().await;
    let _first = registry_package(
        &mut server,
        "first",
        json!({ "shared": "^1.0.0" }),
        "module.exports = require('shared');",
    );
    let _second = registry_package(
        &mut server,
        "second",
        json!({ "shared": "^1.0.0" }),
        "module.exports = require('shared');",
    );
    let mut mocks = Vec::new();
    let mut versions = json!({});
    for version in ["1.0.0", "2.0.0"] {
        let manifest = json!({
            "name": "shared", "version": version, "main": "index.js",
            "dist": { "tarball": format!("{}/shared-{version}.tgz", server.url()) }
        });
        mocks.push(
            server
                .mock("GET", format!("/shared/{version}").as_str())
                .with_body(manifest.to_string())
                .create(),
        );
        mocks.push(
            server
                .mock("GET", format!("/shared-{version}.tgz").as_str())
                .with_body(tarball(manifest.clone(), "module.exports = new Map();"))
                .create(),
        );
        versions[version] = manifest;
    }
    let _metadata = server
        .mock("GET", "/shared")
        .with_body(
            json!({
                "name": "shared", "dist-tags": { "latest": "2.0.0" }, "versions": versions
            })
            .to_string(),
        )
        .create();
    let project = tempdir().unwrap();
    let cache = tempdir().unwrap();
    for target in ["1.0.0", "2.0.0"] {
        fs::write(
            project.path().join("package.json"),
            json!({
                "name": "root", "version": "1.0.0", "private": true,
                "dependencies": { "first": "1.0.0", "second": "1.0.0" },
                "overrides": { "shared": target, "unrelated": { "unused": "2.0.0" } }
            })
            .to_string(),
        )
        .unwrap();
        if target == "2.0.0" {
            run_utoo(project.path(), cache.path(), &server.url(), &["deps"]).await;
            fs::remove_dir_all(project.path().join("node_modules")).unwrap();
        }
        let lock = run_utoo(
            project.path(),
            cache.path(),
            &server.url(),
            &["install", "--ignore-scripts"],
        )
        .await;
        let output = Command::new("node")
            .current_dir(project.path())
            .args([
                "-e",
                r#"
            require("node:assert/strict").equal(require("first"), require("second"));
        "#,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{target}: {output:?}");
        assert_eq!(lock["packages"]["node_modules/shared"]["version"], target);
        assert_eq!(lock["packages"].as_object().unwrap().len(), 4);
    }
}
