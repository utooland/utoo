use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;

use serde_json::Value;
use tempfile::tempdir;

fn utoo() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_utoo"));
    command.env("NO_UPDATE_NOTIFIER", "1");
    command
}

fn serve_publish_registry(status: &str, body: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_string();
    let body = body.to_string();
    let handle = thread::spawn(move || {
        for (response_status, response_body) in
            [("404 Not Found", "{}"), (status.as_str(), body.as_str())]
        {
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

fn run_publish(project: &Path, registry: &str) -> Output {
    let mut command = utoo();
    command
        .current_dir(project)
        .env("HOME", project)
        .env("USERPROFILE", project)
        .env("NPM_TOKEN", "test-token")
        .args(["--json", "--registry", registry, "publish"]);
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
    assert!(value["version"].is_string());
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
    assert!(value["help"].as_str().unwrap().contains("Usage:"));
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
    assert_eq!(value["name"], "fixture");
    assert_eq!(value["dryRun"], true);
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
    assert!(!stderr.contains("LIFECYCLE_STDOUT_MARKER"));
    assert!(!stderr.contains("LIFECYCLE_STDERR_MARKER"));
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["command"], "install");
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], 11);
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
    assert!(!stderr.contains("POSTPUBLISH_STDOUT_MARKER"));
    assert!(!stderr.contains("POSTPUBLISH_STDERR_MARKER"));
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["command"], "publish");
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], 11);
    assert_eq!(
        value["error"]["details"]["completedPackages"][0]["name"],
        "fixture"
    );
    assert_eq!(
        value["error"]["details"]["completedPackages"][0]["version"],
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
    assert_eq!(value["error"]["code"], 3);
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
    assert!(!stderr.contains("LIFECYCLE_STDOUT_MARKER"));
    assert!(!stderr.contains("LIFECYCLE_STDERR_MARKER"));
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["error"]["category"], "local");
    assert_eq!(value["error"]["code"], 11);
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
    assert_eq!(value["error"]["category"], "not_found");
    assert_eq!(value["error"]["code"], 4);
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
    assert_eq!(value["package"], "orphan");
    assert_eq!(value["dependencies"], serde_json::json!([]));
}

#[test]
fn unsupported_json_command_fails_instead_of_printing_human_output() {
    let output = utoo().args(["--json", "ping"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["category"], "usage");
}

#[test]
fn bare_script_json_is_rejected_before_the_script_runs() {
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

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1);
    assert!(!stderr.contains("BARE_SCRIPT_OUTPUT_MARKER"));
    let value: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["error"]["category"], "usage");
    assert_eq!(value["error"]["code"], 2);
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

fn write_cache_entry(home: &Path) -> std::path::PathBuf {
    let entry = home.join(".cache/nm/fixture/1.0.0");
    fs::create_dir_all(&entry).unwrap();
    entry
}

#[test]
fn clean_does_not_prompt_without_a_tty() {
    let home = tempdir().unwrap();
    let entry = write_cache_entry(home.path());
    let output = utoo()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
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
    let entry = write_cache_entry(home.path());
    let output = utoo()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
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
