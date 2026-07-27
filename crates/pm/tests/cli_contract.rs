use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

fn utoo() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_utoo"));
    command.env("NO_UPDATE_NOTIFIER", "1");
    command
}

#[test]
fn pack_json_is_one_clean_document() {
    let project = tempdir().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","files":["index.js"]}"#,
    )
    .unwrap();
    std::fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();

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
fn pack_json_lifecycle_failure_is_one_clean_error_document() {
    let project = tempdir().unwrap();
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","scripts":{"prepack":"node lifecycle-failure.js"},"files":["index.js"]}"#,
    )
    .unwrap();
    std::fs::write(project.path().join("index.js"), "export default 1;\n").unwrap();
    std::fs::write(
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
    std::fs::write(
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
    std::fs::write(
        project.path().join("package.json"),
        r#"{"name":"fixture","version":"1.0.0","scripts":{"build":"node build.js"}}"#,
    )
    .unwrap();
    std::fs::write(
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
fn no_color_disables_ansi_sequences() {
    let project = tempdir().unwrap();
    std::fs::write(
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
    std::fs::write(project.path().join("package.json"), "{}").unwrap();
    let output = utoo()
        .current_dir(project.path())
        .args(["init", "--yes"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}
