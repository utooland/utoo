use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;

use flate2::read::GzDecoder;
use serde_json::Value;
use tar::Archive;
use tempfile::tempdir;

fn utoo() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_utoo"));
    command.env("NO_UPDATE_NOTIFIER", "1");
    command
}

fn assert_same_path(actual: &str, expected: &Path) {
    let actual = Path::new(actual);
    assert!(actual.is_absolute(), "{} is not absolute", actual.display());
    assert_eq!(
        fs::canonicalize(actual).unwrap(),
        fs::canonicalize(expected).unwrap()
    );
}

fn serve_publish_registry_responses(
    responses: &[(&str, &str)],
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responses: Vec<_> = responses
        .iter()
        .map(|(status, body)| (status.to_string(), body.to_string()))
        .collect();
    let handle = thread::spawn(move || {
        for (response_status, response_body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 8192];
            let header_end = loop {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "registry request ended before its headers");
                request.extend_from_slice(&chunk[..read]);
                if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0, "registry request body ended early");
                request.extend_from_slice(&chunk[..read]);
            }

            write!(
                stream,
                "HTTP/1.1 {response_status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            )
            .unwrap();
            stream.flush().unwrap();
        }
    });
    (format!("http://{address}"), handle)
}

fn serve_publish_registry(status: &str, body: &str) -> (String, thread::JoinHandle<()>) {
    serve_publish_registry_responses(&[("404 Not Found", "{}"), (status, body)])
}

fn publish_command(project: &Path, registry: &str) -> Command {
    let mut command = utoo();
    command
        .current_dir(project)
        .env("HOME", project)
        .env("USERPROFILE", project)
        .env("NPM_TOKEN", "test-token");
    for proxy in [
        "ALL_PROXY",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "all_proxy",
        "https_proxy",
        "http_proxy",
    ] {
        command.env_remove(proxy);
    }
    command.arg("--registry").arg(registry);
    command
}

fn run_publish(project: &Path, registry: &str) -> Output {
    let mut command = publish_command(project, registry);
    command.args(["--json", "publish"]);
    command.output().unwrap()
}

fn write_lifecycle_project(project: &Path, exit_code: i32) {
    fs::write(
        project.join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","scripts":{"install":"node lifecycle.js"}}"#,
    )
    .unwrap();
    fs::write(
        project.join("package-lock.json"),
        r#"{
  "name": "fixture",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {"name": "fixture", "version": "1.0.0"}
  }
}"#,
    )
    .unwrap();
    fs::write(
        project.join("lifecycle.js"),
        format!(
            r#"process.stdout.write("LIFECYCLE_STDOUT_MARKER\n");
process.stderr.write("LIFECYCLE_STDERR_MARKER\n");
process.exit({exit_code});
"#
        ),
    )
    .unwrap();
}

#[test]
fn json_version_is_one_machine_document() {
    let output = utoo().args(["--json", "--version"]).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "version");
    assert_eq!(value["ok"], true);
    assert!(value["result"]["version"].is_string());
}

#[test]
fn json_help_is_one_machine_document() {
    let output = utoo().args(["--json", "view", "--help"]).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "help");
    assert_eq!(value["result"]["target"]["command"], "view");
    assert!(value["result"]["text"].as_str().unwrap().contains("Usage:"));
}

#[test]
fn pack_json_is_one_clean_document() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--dry-run"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "pack");
    assert_eq!(value["result"]["name"], "fixture");
    assert_eq!(value["result"]["dryRun"], true);
    assert_eq!(value["result"]["tarballPath"], Value::Null);
}

fn write_scoped_pack_project(project: &Path) {
    fs::create_dir_all(project).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name":"@scope/fixture","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(project.join("index.js"), "export const source = true;\n").unwrap();
}

#[test]
fn pack_destination_is_relative_to_invocation_directory() {
    let project = tempdir().unwrap();
    let source = project.path().join("source");
    write_scoped_pack_project(&source);
    fs::write(
        project.path().join("index.js"),
        "export const caller = true;\n",
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args([
            "--json",
            "pm-pack",
            "source",
            "--pack-destination",
            "artifacts/nested",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["filename"], "scope-fixture-1.0.0.tgz");
    let tarball_path = value["result"]["tarballPath"].as_str().unwrap();
    assert_same_path(
        tarball_path,
        &project
            .path()
            .join("artifacts/nested/scope-fixture-1.0.0.tgz"),
    );
    assert!(!source.join("artifacts").exists());
    assert!(!source.join("scope-fixture-1.0.0.tgz").exists());

    let mut archive = Archive::new(GzDecoder::new(fs::File::open(tarball_path).unwrap()));
    let mut paths = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        let mut contents = String::new();
        entry.read_to_string(&mut contents).unwrap();
        match path.as_str() {
            "package/index.js" => assert_eq!(contents, "export const source = true;\n"),
            "package/package.json" => {
                let manifest: Value = serde_json::from_str(&contents).unwrap();
                assert_eq!(manifest["name"], "@scope/fixture");
            }
            _ => panic!("unexpected packed file: {path}"),
        }
        paths.push(path);
    }
    paths.sort_unstable();
    assert_eq!(paths, ["package/index.js", "package/package.json"]);
}

#[test]
fn pack_destination_accepts_absolute_directory() {
    let project = tempdir().unwrap();
    write_scoped_pack_project(project.path());
    let artifacts = tempdir().unwrap();
    let destination = artifacts.path().join("nested/output");

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--pack-destination"])
        .arg(&destination)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_same_path(
        value["result"]["tarballPath"].as_str().unwrap(),
        &destination.join("scope-fixture-1.0.0.tgz"),
    );
    assert!(!project.path().join("scope-fixture-1.0.0.tgz").exists());
}

#[test]
fn pack_without_destination_writes_to_explicit_package_root() {
    let project = tempdir().unwrap();
    let source = project.path().join("source");
    write_scoped_pack_project(&source);

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "source"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_same_path(
        value["result"]["tarballPath"].as_str().unwrap(),
        &source.join("scope-fixture-1.0.0.tgz"),
    );
    assert!(!project.path().join("scope-fixture-1.0.0.tgz").exists());
}

#[test]
fn pack_destination_dry_run_does_not_create_directory() {
    let project = tempdir().unwrap();
    write_scoped_pack_project(project.path());

    let output = utoo()
        .current_dir(project.path())
        .args([
            "--json",
            "pm-pack",
            "--dry-run",
            "--pack-destination",
            "artifacts/nested",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["dryRun"], true);
    assert_eq!(value["result"]["tarballPath"], Value::Null);
    assert!(!project.path().join("artifacts").exists());
    assert!(!project.path().join("scope-fixture-1.0.0.tgz").exists());
}

#[test]
fn pack_destination_rejects_existing_file() {
    let project = tempdir().unwrap();
    write_scoped_pack_project(project.path());
    let destination = project.path().join("artifacts");
    fs::write(&destination, "keep this file\n").unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--pack-destination", "artifacts"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["command"], "pack");
    assert_eq!(value["ok"], false);
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("artifacts")
    );
    assert_eq!(fs::read_to_string(destination).unwrap(), "keep this file\n");
    assert!(!project.path().join("scope-fixture-1.0.0.tgz").exists());
}

#[test]
fn pack_applies_publish_config_overrides_to_tarball_only() {
    let project = tempdir().unwrap();
    let source_manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "type": "module",
  "main": "./src/index.ts",
  "browser": "./src/browser.ts",
  "types": "./src/index.ts",
  "bin": {
    "fixture": "./src/cli.ts"
  },
  "exports": {
    ".": "./src/index.ts"
  },
  "publishConfig": {
    "name": "fixture-published",
    "main": "./dist/index.js",
    "browser": "./dist/browser.js",
    "types": "./dist/index.d.ts",
    "bin": {
      "fixture": "./dist/cli.js"
    },
    "exports": {
      ".": {
        "import": "./dist/index.js",
        "types": "./dist/index.d.ts"
      }
    }
  },
  "files": []
}"#;
    fs::write(project.path().join("package.json"), source_manifest).unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(
        project.path().join("src/index.ts"),
        "export const source = true;\n",
    )
    .unwrap();
    fs::write(
        project.path().join("src/browser.ts"),
        "export const browserSource = true;\n",
    )
    .unwrap();
    fs::write(
        project.path().join("src/cli.ts"),
        "console.log('source');\n",
    )
    .unwrap();
    fs::create_dir(project.path().join("dist")).unwrap();
    fs::write(
        project.path().join("dist/index.js"),
        "export const compiled = true;\n",
    )
    .unwrap();
    fs::write(
        project.path().join("dist/browser.js"),
        "export const browserCompiled = true;\n",
    )
    .unwrap();
    fs::write(
        project.path().join("dist/index.d.ts"),
        "export declare const compiled: true;\n",
    )
    .unwrap();
    fs::write(
        project.path().join("dist/cli.js"),
        "console.log('compiled');\n",
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["name"], "fixture-published");
    assert_eq!(value["result"]["filename"], "fixture-published-1.0.0.tgz");
    let tarball_path = value["result"]["tarballPath"].as_str().unwrap();
    let decoder = GzDecoder::new(fs::File::open(tarball_path).unwrap());
    let mut archive = Archive::new(decoder);
    let mut packed_manifest = None;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap() == Path::new("package/package.json") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            packed_manifest = Some(serde_json::from_str::<Value>(&contents).unwrap());
            break;
        }
    }
    let packed_manifest = packed_manifest.expect("tarball should contain package/package.json");

    assert_eq!(packed_manifest["name"], "fixture-published");
    assert_eq!(packed_manifest["main"], "./dist/index.js");
    assert_eq!(packed_manifest["browser"], "./dist/browser.js");
    assert_eq!(packed_manifest["types"], "./dist/index.d.ts");
    assert_eq!(packed_manifest["bin"]["fixture"], "./dist/cli.js");
    assert_eq!(packed_manifest["exports"]["."]["import"], "./dist/index.js");
    assert_eq!(
        packed_manifest["exports"]["."]["types"],
        "./dist/index.d.ts"
    );
    assert!(packed_manifest.get("publishConfig").is_none());
    assert_eq!(
        fs::read_to_string(project.path().join("package.json")).unwrap(),
        source_manifest
    );
    let files = value["result"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        files,
        [
            "dist/browser.js",
            "dist/cli.js",
            "dist/index.d.ts",
            "dist/index.js",
            "package.json"
        ]
    );
}

#[test]
fn pack_rejects_empty_publish_config_name() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{
  "name": "fixture",
  "version": "1.0.0",
  "publishConfig": {
    "name": ""
  }
}"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--dry-run"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["command"], "pack");
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["error"]["message"],
        "Missing 'name' field in package.json"
    );
}

#[test]
fn pack_rejects_invalid_publish_config_name() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{
  "name": "fixture",
  "version": "1.0.0",
  "publishConfig": {
    "name": "bad name"
  }
}"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--dry-run"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["command"], "pack");
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["error"]["message"],
        "Invalid package name \"bad name\"."
    );
}

#[test]
fn pack_rejects_missing_source_name_with_publish_config_name() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{
  "version": "1.0.0",
  "publishConfig": {
    "name": "fixture-published"
  }
}"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--dry-run"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["command"], "pack");
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["error"]["message"],
        "Missing 'name' field in package.json"
    );
}

#[test]
fn pack_keeps_publish_config_browser_when_ignored() {
    for ignore_file in [".gitignore", ".npmignore"] {
        let project = tempdir().unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{
  "name": "fixture",
  "version": "1.0.0",
  "publishConfig": {
    "browser": "./dist/browser.js"
  }
}"#,
        )
        .unwrap();
        fs::write(project.path().join(ignore_file), "dist/\n").unwrap();
        fs::create_dir(project.path().join("dist")).unwrap();
        fs::write(
            project.path().join("dist/browser.js"),
            "export const browser = true;\n",
        )
        .unwrap();

        let output = utoo()
            .current_dir(project.path())
            .args(["--json", "pm-pack", "--dry-run"])
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{ignore_file}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let files = value["result"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(files, ["dist/browser.js", "package.json"], "{ignore_file}");
    }
}

#[test]
fn install_json_lifecycle_success_is_one_clean_document() {
    let project = tempdir().unwrap();
    write_lifecycle_project(project.path(), 0);

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "install"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(!stdout.contains("LIFECYCLE_STDOUT_MARKER"));
    assert!(!stdout.contains("LIFECYCLE_STDERR_MARKER"));
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["command"], "install");
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["operation"], "install");
}

#[test]
fn install_json_lifecycle_failure_is_one_stable_error_document() {
    let project = tempdir().unwrap();
    write_lifecycle_project(project.path(), 42);

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "install"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(11),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "install");
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], "script_failed");
    assert_eq!(value["error"]["exitCode"], 11);
    let details = &value["error"]["details"];
    assert_eq!(details["kind"], "lifecycle");
    let execution = &details["executions"][0];
    assert_eq!(execution["package"], "fixture");
    assert_eq!(execution["event"], "install");
    assert_eq!(execution["command"], "node lifecycle.js");
    assert_same_path(
        execution["cwd"].as_str().expect("cwd should be a string"),
        project.path(),
    );
    assert_eq!(execution["status"], "failed");
    assert_eq!(execution["exitCode"], 42);
    assert_eq!(execution["stdout"]["tail"], "LIFECYCLE_STDOUT_MARKER\n");
    assert_eq!(execution["stderr"]["tail"], "LIFECYCLE_STDERR_MARKER\n");
    assert_eq!(execution["stdout"]["bytes"], 24);
    assert_eq!(execution["stderr"]["bytes"], 24);
    assert_eq!(execution["stdout"]["truncated"], false);
    assert_eq!(execution["stderr"]["truncated"], false);
}

#[test]
fn install_human_lifecycle_failure_still_streams_script_output() {
    let project = tempdir().unwrap();
    write_lifecycle_project(project.path(), 42);

    let output = utoo()
        .current_dir(project.path())
        .arg("install")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(42));
    assert!(String::from_utf8_lossy(&output.stdout).contains("LIFECYCLE_STDOUT_MARKER"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("LIFECYCLE_STDERR_MARKER"));
    assert!(stderr.contains("Custom script execution failed with exit code: 42"));
}

#[test]
fn install_json_migration_does_not_emit_a_human_summary() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(project.path().join("pnpm-workspace.yaml"), "packages: []\n").unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "install", "--from", "pnpm", "--ignore-scripts"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(!stdout.contains("pnpm"));
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["command"], "install");
}

#[test]
fn publish_json_reports_registry_commit_when_postpublish_fails() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{
  "name": "fixture",
  "version": "1.0.0",
  "files": ["index.js"],
  "scripts": {"postpublish": "node postpublish.js"}
}"#,
    )
    .unwrap();
    fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();
    fs::write(
        project.path().join("postpublish.js"),
        r#"process.stdout.write("POSTPUBLISH_STDOUT_MARKER\n");
process.stderr.write("POSTPUBLISH_STDERR_MARKER\n");
process.exit(42);
"#,
    )
    .unwrap();
    let (registry, server) = serve_publish_registry("201 Created", "{}");

    let output = run_publish(project.path(), &registry);
    server.join().unwrap();

    assert_eq!(
        output.status.code(),
        Some(11),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["command"], "publish");
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], "script_failed");
    assert_eq!(value["error"]["exitCode"], 11);
    let details = &value["error"]["details"];
    assert_eq!(details["kind"], "lifecycle");
    let execution = &details["executions"][0];
    assert_eq!(execution["package"], "fixture");
    assert_eq!(execution["event"], "postpublish");
    assert_eq!(execution["command"], "node postpublish.js");
    assert_eq!(execution["exitCode"], 42);
    assert_eq!(execution["stdout"]["tail"], "POSTPUBLISH_STDOUT_MARKER\n");
    assert_eq!(execution["stderr"]["tail"], "POSTPUBLISH_STDERR_MARKER\n");
    assert_eq!(
        value["error"]["partialResult"]["packages"][0]["name"],
        "fixture"
    );
    assert_eq!(
        value["error"]["partialResult"]["packages"][0]["version"],
        "1.0.0"
    );
}

#[test]
fn publish_json_classifies_forbidden_as_auth() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();
    let (registry, server) = serve_publish_registry("403 Forbidden", r#"{"error":"forbidden"}"#);

    let output = run_publish(project.path(), &registry);
    server.join().unwrap();

    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["error"]["category"], "auth");
    assert_eq!(value["error"]["code"], "registry_publish_failed");
    assert_eq!(value["error"]["exitCode"], 3);
}

#[test]
fn publish_human_reports_completed_workspace_before_later_failure() {
    let project = tempdir().unwrap();
    fs::create_dir_all(project.path().join("A")).unwrap();
    fs::create_dir_all(project.path().join("B")).unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["A","B"]}"#,
    )
    .unwrap();
    fs::write(
        project.path().join("A/package.json"),
        r#"{"name":"fixture-a","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        project.path().join("B/package.json"),
        r#"{
  "name": "fixture-b",
  "version": "1.0.0",
  "private": true,
  "dependencies": {"fixture-a": "workspace:*"}
}"#,
    )
    .unwrap();
    let (registry, server) = serve_publish_registry("201 Created", "{}");

    let output = publish_command(project.path(), &registry)
        .args(["--filter", "*", "publish"])
        .output()
        .unwrap();
    server.join().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("+ fixture-a@1.0.0"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("marked as private"));
}

#[test]
fn pack_json_lifecycle_failure_is_one_clean_error_document() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","scripts":{"prepack":"node lifecycle-failure.js"},"files":["index.js"]}"#,
    )
    .unwrap();
    fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();
    fs::write(
        project.path().join("lifecycle-failure.js"),
        r#"process.stdout.write("LIFECYCLE_STDOUT_MARKER\n");
process.stderr.write("LIFECYCLE_STDERR_MARKER\n");
process.exit(42);
"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "pm-pack", "--dry-run"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], "script_failed");
    assert_eq!(value["error"]["exitCode"], 11);
    let details = &value["error"]["details"];
    assert_eq!(details["kind"], "lifecycle");
    let execution = &details["executions"][0];
    assert_eq!(execution["package"], "fixture");
    assert_eq!(execution["event"], "prepack");
    assert_eq!(execution["command"], "node lifecycle-failure.js");
    assert_eq!(execution["exitCode"], 42);
    assert_eq!(execution["stdout"]["tail"], "LIFECYCLE_STDOUT_MARKER\n");
    assert_eq!(execution["stderr"]["tail"], "LIFECYCLE_STDERR_MARKER\n");
}

#[test]
fn json_error_is_structured_and_uses_stable_exit_code() {
    let project = tempdir().unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "config", "get", "rfc-key-that-does-not-exist"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["command"], "config");
    assert_eq!(value["subcommand"], "get");
    assert_eq!(value["error"]["category"], "not_found");
    assert_eq!(value["error"]["code"], "not_found");
    assert_eq!(value["error"]["exitCode"], 4);
}

#[cfg(unix)]
fn node_gyp_rebuild_command(project: &Path) -> Command {
    use std::os::unix::fs::PermissionsExt;

    let manifest = serde_json::json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {"native-b": "1.0.0"},
        "optionalDependencies": {"native-a": "1.0.0"},
    });
    fs::write(project.join("package.json"), manifest.to_string()).unwrap();
    fs::write(
        project.join("package-lock.json"),
        serde_json::json!({
            "name": "fixture",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "packages": {
                "": manifest,
                "node_modules/native-a": {
                    "version": "1.0.0",
                    "hasInstallScript": true,
                    "optional": true,
                },
                "node_modules/native-b": {
                    "version": "1.0.0",
                    "hasInstallScript": true,
                },
            },
        })
        .to_string(),
    )
    .unwrap();
    for name in ["native-a", "native-b"] {
        let package = project.join("node_modules").join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "scripts": {"install": "exit 99"},
            })
            .to_string(),
        )
        .unwrap();
        fs::write(package.join("binding.gyp"), "{}").unwrap();
    }
    let bin = project.join("bin");
    fs::create_dir(&bin).unwrap();
    let fake_ut = bin.join("ut");
    fs::write(
        &fake_ut,
        r#"#!/bin/sh
printf '%s\n' attempt >> "$UTOO_TEST_NODE_GYP_ATTEMPTS"
printf '%s\n' "$@" > "$UTOO_TEST_NODE_GYP_ARGS"
printf '%s\n' BOOTSTRAP_STDOUT_MARKER
printf '%s\n' \
    'Error: BOOTSTRAP_STDERR_MARKER' \
    '    at fetch (registry.js:42:7)' \
    'Full logs saved to: /tmp/child.log' >&2
exit 1
"#,
    )
    .unwrap();
    fs::set_permissions(fake_ut, fs::Permissions::from_mode(0o755)).unwrap();

    let mut command = utoo();
    command
        .current_dir(project)
        .env("PATH", bin)
        .env("UTOO_TEST_NODE_GYP_ATTEMPTS", project.join("attempts"))
        .env("UTOO_TEST_NODE_GYP_ARGS", project.join("bootstrap-args"))
        .args(["--registry", "http://127.0.0.1:9", "rebuild"]);
    command
}

#[cfg(unix)]
#[test]
fn rebuild_node_gyp_bootstrap_preserves_cli_registry() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join(".npmrc"),
        "registry=https://project.example.com\n",
    )
    .unwrap();
    let output = node_gyp_rebuild_command(project.path())
        .env("UTOO_REGISTRY", "https://environment.example.com")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(11));
    let args = fs::read_to_string(project.path().join("bootstrap-args")).unwrap();
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        ["i", "-g", "node-gyp", "--registry", "http://127.0.0.1:9"]
    );
}

#[cfg(unix)]
#[test]
fn rebuild_json_node_gyp_failure_is_one_structured_error_document() {
    let project = tempdir().unwrap();
    let output = node_gyp_rebuild_command(project.path())
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(11));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["command"], "rebuild");
    assert_eq!(value["error"]["code"], "process_failed");
    assert!(
        !value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("BOOTSTRAP_")
    );
    let details = &value["error"]["details"];
    assert_eq!(details["kind"], "process");
    let execution = &details["execution"];
    assert_eq!(
        execution["command"],
        "ut i -g node-gyp --registry http://127.0.0.1:9"
    );
    assert_same_path(execution["cwd"].as_str().unwrap(), project.path());
    assert_eq!(execution["status"], "failed");
    assert_eq!(execution["exitCode"], 1);
    assert_eq!(execution["stdout"]["tail"], "BOOTSTRAP_STDOUT_MARKER\n");
    assert_eq!(
        execution["stderr"]["tail"],
        concat!(
            "Error: BOOTSTRAP_STDERR_MARKER\n",
            "    at fetch (registry.js:42:7)\n",
            "Full logs saved to: /tmp/child.log\n",
        )
    );
    assert_eq!(execution["stdout"]["truncated"], false);
    assert_eq!(execution["stderr"]["truncated"], false);
    assert_eq!(
        fs::read_to_string(project.path().join("attempts")).unwrap(),
        "attempt\n"
    );
}

#[cfg(unix)]
#[test]
fn rebuild_human_node_gyp_failure_renders_diagnostics() {
    let project = tempdir().unwrap();
    let output = node_gyp_rebuild_command(project.path()).output().unwrap();

    assert_eq!(output.status.code(), Some(11));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Failed to install node-gyp globally: exit status: 1"));
    assert!(stderr.contains("--- stdout ---\nBOOTSTRAP_STDOUT_MARKER\n"));
    assert!(stderr.contains("--- stderr ---\nError: BOOTSTRAP_STDERR_MARKER\n"));
    assert!(stderr.contains("    at fetch (registry.js:42:7)\n"));
    assert!(stderr.contains("Full logs saved to: /tmp/child.log"));
    assert_eq!(
        fs::read_to_string(project.path().join("attempts")).unwrap(),
        "attempt\n"
    );
}

#[cfg(unix)]
#[test]
fn rebuild_human_optional_node_gyp_failure_renders_diagnostics() {
    let project = tempdir().unwrap();
    let mut command = node_gyp_rebuild_command(project.path());
    let manifest_path = project.path().join("package.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["dependencies"] = serde_json::json!({});
    manifest["optionalDependencies"]["native-b"] = serde_json::json!("1.0.0");
    fs::write(manifest_path, manifest.to_string()).unwrap();
    let lock_path = project.path().join("package-lock.json");
    let mut lock: Value = serde_json::from_slice(&fs::read(&lock_path).unwrap()).unwrap();
    lock["packages"][""] = manifest;
    lock["packages"]["node_modules/native-b"]["optional"] = serde_json::json!(true);
    fs::write(lock_path, lock.to_string()).unwrap();

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Optional dependency script failed (ignored)"));
    assert!(stderr.contains("BOOTSTRAP_STDERR_MARKER"));
    assert!(stderr.contains("    at fetch (registry.js:42:7)\n"));
    assert_eq!(
        fs::read_to_string(project.path().join("attempts")).unwrap(),
        "attempt\n"
    );
}

#[test]
fn list_json_emits_a_document_for_a_disconnected_package() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package-lock.json"),
        r#"{
  "name": "fixture",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "packages": {
    "": {"name": "fixture", "version": "1.0.0"},
    "node_modules/orphan": {"name": "orphan", "version": "1.0.0"}
  }
}"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "list", "orphan"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["command"], "list");
    assert_eq!(value["result"]["package"], "orphan");
    assert_eq!(value["result"]["paths"], serde_json::json!([]));
    assert_eq!(value["result"]["truncated"], false);
}

#[test]
fn completions_json_is_a_machine_document() {
    let output = utoo()
        .args(["--json", "completions", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "completions");
    assert_eq!(value["result"]["shell"], "bash");
    assert!(value["result"]["script"].as_str().unwrap().contains("utoo"));
}

#[test]
fn bare_script_json_captures_the_script_result() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","scripts":{"build":"node build.js"}}"#,
    )
    .unwrap();
    fs::write(
        project.path().join("build.js"),
        r#"process.stdout.write("BARE_SCRIPT_OUTPUT_MARKER\n");"#,
    )
    .unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "build"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["command"], "run");
    assert_eq!(value["result"]["script"], "build");
    assert_eq!(value["result"]["executions"][0]["event"], "build");
    assert_eq!(
        value["result"]["executions"][0]["stdout"]["tail"],
        "BARE_SCRIPT_OUTPUT_MARKER\n"
    );
}

#[test]
fn invalid_json_invocation_returns_a_json_usage_error() {
    let output = utoo()
        .args(["--json", "view", "--definitely-invalid"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["category"], "usage");
}

#[test]
fn forwarded_json_after_delimiter_does_not_change_help_format() {
    let output = utoo()
        .args(["x", "--help", "--", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
}

#[test]
fn init_does_not_prompt_without_a_tty() {
    let project = tempdir().unwrap();
    let mut command = utoo();
    command.current_dir(project.path()).arg("init");
    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to prompt for package metadata")
    );
    assert!(!project.path().join("package.json").exists());
}

#[test]
fn init_yes_uses_defaults_without_a_tty() {
    let project = tempdir().unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["init", "--yes"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.path().join("package.json").exists());
}

#[test]
fn init_json_returns_the_created_manifest_identity() {
    let project = tempdir().unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "init", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "init");
    assert_eq!(value["result"]["version"], "1.0.0");
    assert_same_path(
        value["result"]["path"]
            .as_str()
            .expect("path should be a string"),
        &project.path().join("package.json"),
    );
}

#[test]
fn login_json_is_a_structured_interactive_error() {
    let output = utoo().args(["--json", "login"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["command"], "login");
    assert_eq!(value["error"]["code"], "interactive_required");
    assert_eq!(value["error"]["exitCode"], 2);
}

#[test]
fn config_json_uses_command_and_subcommand_fields() {
    let project = tempdir().unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "config", "set", "fixture-key", "fixture-value"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "config");
    assert_eq!(value["subcommand"], "set");
    assert_eq!(value["result"]["values"]["fixture-key"], "fixture-value");
    assert_eq!(value["result"]["scope"], "local");
}

#[cfg(unix)]
#[test]
fn execute_json_captures_child_output() {
    use std::os::unix::fs::PermissionsExt;

    let project = tempdir().unwrap();
    let bin_dir = project.path().join("node_modules/.bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let executable = bin_dir.join("fixture-tool");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf 'EXECUTE_STDOUT_MARKER\\n'\nprintf 'EXECUTE_STDERR_MARKER\\n' >&2\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();

    let output = utoo()
        .current_dir(project.path())
        .args(["--json", "execute", "fixture-tool"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "execute");
    assert_eq!(value["result"]["source"], "local");
    assert_eq!(value["result"]["execution"]["status"], "succeeded");
    assert_eq!(
        value["result"]["execution"]["stdout"]["tail"],
        "EXECUTE_STDOUT_MARKER\n"
    );
    assert_eq!(
        value["result"]["execution"]["stderr"]["tail"],
        "EXECUTE_STDERR_MARKER\n"
    );
}

fn write_cache_entry(cache_dir: &Path) -> std::path::PathBuf {
    let entry = cache_dir.join("fixture/1.0.0");
    fs::create_dir_all(&entry).unwrap();
    entry
}

#[test]
fn clean_does_not_prompt_without_a_tty() {
    let home = tempdir().unwrap();
    let cache_dir = home.path().join("cache");
    let entry = write_cache_entry(&cache_dir);
    let output = utoo()
        .env("UTOO_CACHE_DIR", &cache_dir)
        .arg("clean")
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("utoo clean --yes"));
    assert!(entry.exists());
}

#[test]
fn clean_yes_deletes_without_a_prompt() {
    let home = tempdir().unwrap();
    let cache_dir = home.path().join("cache");
    let entry = write_cache_entry(&cache_dir);
    let output = utoo()
        .env("UTOO_CACHE_DIR", &cache_dir)
        .args(["clean", "--yes"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Confirm to delete"));
    assert!(!entry.exists());
}

#[test]
fn clean_json_reports_deleted_entries() {
    let home = tempdir().unwrap();
    let cache_dir = home.path().join("cache");
    let entry = write_cache_entry(&cache_dir);
    let output = utoo()
        .env("UTOO_CACHE_DIR", &cache_dir)
        .args(["--json", "clean", "--yes"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "clean");
    assert_eq!(value["result"]["deleted"][0]["name"], "fixture");
    assert_eq!(value["result"]["deleted"][0]["version"], "1.0.0");
    assert_eq!(value["result"]["summary"]["matched"], 1);
    assert_eq!(value["result"]["summary"]["deleted"], 1);
    assert!(!entry.exists());
}

#[test]
fn no_color_disables_ansi_sequences() {
    let project = tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["--no-color", "pm-pack", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.windows(2).any(|bytes| bytes == b"\x1b["));
    assert!(!output.stderr.windows(2).any(|bytes| bytes == b"\x1b["));
}

#[test]
fn precondition_errors_have_their_own_exit_code() {
    let project = tempdir().unwrap();
    fs::write(project.path().join("package.json"), "{}").unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["init", "--yes"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn completions_auto_detects_shell_from_shell_env() {
    let output = utoo()
        .env("SHELL", "/bin/zsh")
        .arg("completions")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("#compdef utoo"),
        "expected a zsh completion script, got: {}",
        stdout
    );
    assert!(
        stdout.contains("install"),
        "completion script should include subcommands"
    );
}

#[test]
fn completions_without_shell_env_reports_error() {
    let output = utoo()
        .env_remove("SHELL")
        .arg("completions")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr);
    assert!(
        stderr.contains("Could not detect shell"),
        "expected a shell-detection error, stderr: {}",
        stderr
    );
}

#[test]
fn completions_with_unsupported_shell_reports_error() {
    let output = utoo()
        .env("SHELL", "/bin/tcsh")
        .arg("completions")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr);
    assert!(
        stderr.contains("Could not detect shell"),
        "expected a shell-detection error, stderr: {}",
        stderr
    );
}
