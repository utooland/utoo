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

#[test]
fn pack_applies_publish_config_overrides_to_tarball_only() {
    let project = tempdir().unwrap();
    let source_manifest = r#"{
  "name": "fixture",
  "version": "1.0.0",
  "type": "module",
  "main": "./src/index.ts",
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
            "dist/cli.js",
            "dist/index.d.ts",
            "dist/index.js",
            "package.json"
        ]
    );
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
