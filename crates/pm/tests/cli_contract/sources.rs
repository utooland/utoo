//! Portable counterparts of the #3368 review reproductions. Each invocation
//! has its own project/config/cache; registry traffic stays on loopback.
use super::{fs, tempdir, utoo};
use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::path::Path;
use std::process::{Command, Output};

fn archive(manifest: &Value, marker: &str) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, body) in [
        ("package/package.json", manifest.to_string()),
        ("package/marker.txt", marker.to_string()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, body.as_bytes()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn integrity(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

fn command(project: &Path, cache: &Path, registry: &str) -> Command {
    let mut cmd = utoo();
    cmd.current_dir(project)
        .env("CI", "1")
        .env("UTOO_SELF_PIN", "0")
        .env("XDG_CONFIG_HOME", project)
        .env("HOME", project)
        .env("USERPROFILE", project)
        .env_remove("UTOO_FORCE_UPDATE")
        .env_remove("NPM_TOKEN")
        .env_remove("NODE_AUTH_TOKEN")
        .args(["--quiet", "--json", "--registry", registry, "--cache-dir"])
        .arg(cache);
    for proxy in [
        "ALL_PROXY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "all_proxy",
        "http_proxy",
        "https_proxy",
    ] {
        cmd.env_remove(proxy);
    }
    cmd
}

fn project_at(path: &Path, manifest: Value, packages: Value) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("package.json"), manifest.to_string()).unwrap();
    let mut entries = packages.as_object().unwrap().clone();
    entries.insert(String::new(), manifest.clone());
    fs::write(
        path.join("package-lock.json"),
        json!({"name": manifest["name"], "version": "1.0.0", "lockfileVersion": 3,
            "requires": true, "packages": entries})
        .to_string(),
    )
    .unwrap();
}

fn locked_project(path: &Path, url: &str, digest: Option<&str>, optional: bool) {
    let mut root = json!({"name": "root", "version": "1.0.0"});
    root[if optional {
        "optionalDependencies"
    } else {
        "dependencies"
    }] = json!({"fixture": "1.0.0"});
    let mut entry = json!({"name": "fixture", "version": "1.0.0", "resolved": url,
        "hasInstallScript": true});
    if let Some(digest) = digest {
        entry["integrity"] = digest.into();
    }
    if optional {
        entry["optional"] = true.into();
    }
    project_at(path, root, json!({"node_modules/fixture": entry}));
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _: Value = serde_json::from_slice(&output.stdout).unwrap();
}

#[test]
fn failed_optional_reinstall_does_not_run_retained_hooks() {
    let mut registry = mockito::Server::new();
    let fixture = tempdir().unwrap();
    let project = fixture.path().join("project");
    let manifest = json!({"name":"fixture","version":"1.0.0","scripts":{"install":"node -e \"require('fs').writeFileSync('../../hook-ran', 'yes')\""}});
    let tarball = registry
        .mock("GET", "/fixture.tgz")
        .with_body(archive(&manifest, "invalid"))
        .expect_at_least(1)
        .create();
    locked_project(
        &project,
        &format!("{}/fixture.tgz", registry.url()),
        Some(&integrity(b"wrong")),
        true,
    );
    let installed = project.join("node_modules/fixture");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("package.json"), manifest.to_string()).unwrap();
    assert_success(
        &command(&project, &fixture.path().join("cache"), &registry.url())
            .arg("install")
            .output()
            .unwrap(),
    );
    assert!(!project.join("hook-ran").exists());
    assert!(!installed.exists());
    tarball.assert();
}

#[test]
fn integrity_rejects_corruption_before_scripts_and_optional_dependencies_skip_it() {
    let mut registry = mockito::Server::new();
    let fixture = tempdir().unwrap();
    let bytes = archive(
        &json!({"name":"fixture", "version":"1.0.0", "scripts":{
        "install": "node -e \"require('fs').writeFileSync('script-ran', 'yes')\""}}),
        "untrusted",
    );
    let tarball = registry
        .mock("GET", "/fixture.tgz")
        .with_body(bytes)
        .expect_at_least(1)
        .create();
    for optional in [false, true] {
        let project = fixture
            .path()
            .join(if optional { "optional" } else { "required" });
        locked_project(
            &project,
            &format!("{}/fixture.tgz", registry.url()),
            Some(&integrity(b"wrong bytes")),
            optional,
        );
        let output = command(&project, &fixture.path().join("cache"), &registry.url())
            .arg("install")
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            optional,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!project.join("node_modules/fixture/package.json").exists());
        assert!(!project.join("node_modules/fixture/script-ran").exists());
    }
    tarball.assert();
}

#[test]
fn verified_cache_reuses_matching_digest_and_rejects_a_changed_digest() {
    let mut registry = mockito::Server::new();
    let fixture = tempdir().unwrap();
    let project = fixture.path().join("project");
    let cache = fixture.path().join("cache");
    let bytes = archive(&json!({"name":"fixture","version":"1.0.0"}), "verified");
    let url = format!("{}/fixture.tgz", registry.url());
    let tarball = registry
        .mock("GET", "/fixture.tgz")
        .with_body(bytes.clone())
        .expect(2)
        .create();
    for _ in 0..2 {
        locked_project(&project, &url, Some(&integrity(&bytes)), false);
        assert_success(
            &command(&project, &cache, &registry.url())
                .args(["install", "--ignore-scripts"])
                .output()
                .unwrap(),
        );
        assert_eq!(
            fs::read_to_string(project.join("node_modules/fixture/marker.txt")).unwrap(),
            "verified"
        );
        fs::remove_dir_all(project.join("node_modules")).unwrap();
    }
    locked_project(&project, &url, Some(&integrity(b"different")), false);
    let output = command(&project, &cache, &registry.url())
        .arg("install")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!project.join("node_modules/fixture/package.json").exists());
    tarball.assert();
}

#[test]
fn direct_file_tarball_checks_lock_integrity() {
    let fixture = tempdir().unwrap();
    let tarball = fixture.path().join("fixture.tgz");
    fs::write(
        &tarball,
        archive(&json!({"name":"fixture","version":"1.0.0"}), "file"),
    )
    .unwrap();
    let project = fixture.path().join("project");
    locked_project(
        &project,
        &format!("file:{}", tarball.display()),
        Some(&integrity(b"wrong")),
        false,
    );
    let output = command(
        &project,
        &fixture.path().join("cache"),
        "http://127.0.0.1:9",
    )
    .arg("install")
    .output()
    .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("integrity"),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.join("node_modules/fixture/package.json").exists());
}

#[test]
fn global_root_checks_registry_integrity() {
    let mut registry = mockito::Server::new();
    let fixture = tempdir().unwrap();
    let manifest = registry.mock("GET", "/fixture").with_body(json!({
        "name":"fixture", "dist-tags":{"latest":"1.0.0"}, "versions":{"1.0.0":{"name":"fixture", "version":"1.0.0", "dist":{
            "tarball":format!("{}/fixture.tgz", registry.url()), "integrity":integrity(b"wrong")}}}
    }).to_string()).create();
    let tarball = registry
        .mock("GET", "/fixture.tgz")
        .with_body(archive(
            &json!({"name":"fixture","version":"1.0.0"}),
            "global",
        ))
        .expect_at_least(1)
        .create();
    let prefix = fixture.path().join("prefix");
    let output = command(
        fixture.path(),
        &fixture.path().join("cache"),
        &registry.url(),
    )
    .args(["install", "fixture@1.0.0", "--global", "--prefix"])
    .arg(&prefix)
    .output()
    .unwrap();
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("integrity"),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    manifest.assert();
    tarball.assert();
}

#[test]
fn clean_removes_legacy_and_verified_package_caches() {
    let fixture = tempdir().unwrap();
    let cache = fixture.path().join("cache");
    let legacy = cache.join("fixture/1.0.0");
    let verified = fixture
        .path()
        .join("cache.utoo-v2/packages/fixture/1.0.0/digest");
    for path in [&legacy, &verified] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("_resolved"), "").unwrap();
    }
    let output = command(fixture.path(), &cache, "http://127.0.0.1:9")
        .args(["clean", "fixture@1.0.0", "--yes"])
        .output()
        .unwrap();
    assert_success(&output);
    assert!(!legacy.exists());
    assert!(!verified.exists());
}

#[test]
fn source_identity_separates_caches_and_existing_targets_without_digests() {
    let fixture = tempdir().unwrap();
    let project = fixture.path().join("project");
    let cache = fixture.path().join("cache");
    let mut first = mockito::Server::new();
    let mut second = mockito::Server::new();
    for (registry, marker) in [(&mut first, "registry A"), (&mut second, "registry B")] {
        let tarball = registry
            .mock("GET", "/fixture.tgz")
            .with_body(archive(
                &json!({"name":"fixture", "version":"1.0.0"}),
                marker,
            ))
            .expect(1)
            .create();
        for _ in 0..2 {
            locked_project(
                &project,
                &format!("{}/fixture.tgz", registry.url()),
                None,
                false,
            );
            assert_success(
                &command(&project, &cache, &registry.url())
                    .args(["install", "--ignore-scripts"])
                    .output()
                    .unwrap(),
            );
            assert_eq!(
                fs::read_to_string(project.join("node_modules/fixture/marker.txt")).unwrap(),
                marker
            );
        }
        tarball.assert();
    }
}

#[test]
fn git_lock_recovers_exact_commit_with_empty_cache() {
    let fixture = tempdir().unwrap();
    let repo = fixture.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&repo)
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    git(&["init", "--quiet"]);
    fs::write(
        repo.join("package.json"),
        r#"{"name":"fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(repo.join("marker.txt"), "locked").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "locked"]);
    let commit = git(&["rev-parse", "HEAD"]);
    fs::write(repo.join("marker.txt"), "new head").unwrap();
    git(&["commit", "--quiet", "-am", "advance"]);
    let url = format!(
        "git+{}#{commit}",
        reqwest::Url::from_directory_path(&repo).unwrap()
    );
    let project = fixture.path().join("project");
    let cache = fixture.path().join("cache");
    locked_project(&project, &url, None, false);
    let lock_before = fs::read(project.join("package-lock.json")).unwrap();
    for _ in 0..2 {
        assert_success(
            &command(&project, &cache, "http://127.0.0.1:9")
                .args(["install", "--ignore-scripts"])
                .output()
                .unwrap(),
        );
        assert_eq!(
            fs::read_to_string(project.join("node_modules/fixture/marker.txt")).unwrap(),
            "locked"
        );
        assert_eq!(
            fs::read(project.join("package-lock.json")).unwrap(),
            lock_before
        );
        fs::remove_dir_all(project.join("node_modules")).unwrap();
        // Cache reuse must work after the source is unavailable.
        if repo.exists() {
            fs::rename(&repo, fixture.path().join("offline-repo")).unwrap();
        }
    }
}

#[test]
fn git_lock_does_not_fall_back_to_a_branch() {
    let fixture = tempdir().unwrap();
    let project = fixture.path().join("project");
    locked_project(
        &project,
        "git+https://example.invalid/fixture.git#main",
        None,
        false,
    );
    let output = command(
        &project,
        &fixture.path().join("cache"),
        "http://127.0.0.1:9",
    )
    .args(["install", "--ignore-scripts"])
    .output()
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("full commit"));
    assert!(!project.join("node_modules/fixture/package.json").exists());
}
