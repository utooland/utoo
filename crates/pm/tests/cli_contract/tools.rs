//! Controlled tool bootstrap: no public registry, Python or native compiler.
use super::sources::{archive_with_files, assert_success, command, integrity};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn package(
    server: &mut mockito::Server,
    name: &str,
    manifest: Value,
    files: &[(&str, &str)],
) -> Vec<mockito::Mock> {
    let mut entries = vec![("package.json", manifest.to_string())];
    entries.extend(files.iter().map(|(path, body)| (*path, body.to_string())));
    let bytes = archive_with_files(&entries);
    let mut version = manifest;
    version["dist"] =
        json!({"tarball":format!("{}/{name}.tgz",server.url()),"integrity":integrity(&bytes)});
    vec![
        server
            .mock("GET", format!("/{name}").as_str())
            .with_body(
                json!({"name":name,"dist-tags":{"latest":"1.0.0"},"versions":{"1.0.0":version}})
                    .to_string(),
            )
            .expect_at_least(1)
            .create(),
        server
            .mock("GET", format!("/{name}.tgz").as_str())
            .with_body(bytes)
            .expect(1)
            .create(),
    ]
}

fn isolated_path(root: &Path) -> std::ffi::OsString {
    let bin = root.join("path");
    fs::create_dir(&bin).unwrap();
    let node = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(node.status.success());
    let node = String::from_utf8(node.stdout).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(node.trim(), bin.join("node")).unwrap();
    #[cfg(windows)]
    fs::copy(node.trim(), bin.join("node.exe")).unwrap();
    let fake_ut = bin.join("ut");
    fs::write(
        &fake_ut,
        "#!/bin/sh\necho PATH_SHADOW_WAS_EXECUTED >&2\nexit 99\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_ut, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let original = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&original).filter(|dir| {
            !["node-gyp", "node-gyp.cmd", "node-gyp.exe"]
                .iter()
                .any(|name| dir.join(name).exists())
        })),
    )
    .unwrap()
}

fn bootstrap_case(retry: bool) {
    let mut registry = mockito::Server::new();
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let hooks = |name: &str| {
        json!({
        "name":name,"version":"1.0.0","hasInstallScript":true,"scripts":{
            "preinstall":format!("node-gyp {name}:preinstall"),
            "install":format!("node-gyp {name}:install"),
            "postinstall":format!("node-gyp {name}:postinstall")}})
    };
    let mut tool = hooks("node-gyp");
    tool["bin"] = json!({"node-gyp":"bin/node-gyp.js"});
    tool["dependencies"] = json!({"tool-dep":"1.0.0"});
    tool["devDependencies"] = json!({"never-download":"1.0.0"});
    let program = r#"#!/usr/bin/env node
require('tool-dep');
const fs = require('fs');
const event = process.argv[2];
fs.appendFileSync(process.env.TOOL_EVENTS, event + '\n');
if (process.env.TOOL_FAIL_ONCE && event === 'node-gyp:preinstall' && !fs.existsSync(process.env.TOOL_FAIL_ONCE)) {
  fs.writeFileSync(process.env.TOOL_FAIL_ONCE, 'failed');
  process.exit(12);
}
"#;
    let mut mocks = package(
        &mut registry,
        "node-gyp",
        tool,
        &[("binding.gyp", "{}"), ("bin/node-gyp.js", program)],
    );
    mocks.extend(package(
        &mut registry,
        "tool-dep",
        hooks("tool-dep"),
        &[
            ("binding.gyp", "{}"),
            ("index.js", "module.exports = true;\n"),
        ],
    ));
    for name in ["native-a", "native-b"] {
        mocks.extend(package(
            &mut registry,
            name,
            hooks(name),
            &[("binding.gyp", "{}")],
        ));
    }
    let mut project = json!({"name":"root","version":"1.0.0"});
    project[if retry {
        "optionalDependencies"
    } else {
        "dependencies"
    }] = json!({"native-a":"1.0.0","native-b":"1.0.0"});
    fs::write(root.join("package.json"), project.to_string()).unwrap();
    let prefix = root.join("prefix");
    let events = root.join("events");
    let mut cmd = command(root, &root.join("cache"), &registry.url());
    cmd.env("PATH", isolated_path(root))
        .env("UTOO_PREFIX", &prefix)
        .env("TOOL_EVENTS", &events);
    if retry {
        cmd.env("TOOL_FAIL_ONCE", root.join("failed-once"));
    }
    let output = cmd.arg("install").output().unwrap();
    assert_success(&output);
    let events = fs::read_to_string(events).unwrap();
    let events: Vec<_> = events.lines().collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "node-gyp:preinstall")
            .count(),
        if retry { 2 } else { 1 }
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "node-gyp:postinstall")
            .count(),
        1
    );
    let bootstrap_done = events
        .iter()
        .position(|event| *event == "node-gyp:postinstall")
        .unwrap();
    for (index, event) in events.iter().enumerate() {
        if event.starts_with("native-") {
            assert!(index > bootstrap_done, "{events:?}");
        }
    }
    assert!(events.contains(&"tool-dep:preinstall"));
    assert!(events.contains(&"tool-dep:install"));
    assert!(events.contains(&"tool-dep:postinstall"));
    assert!(events.contains(&"native-a:postinstall"));
    assert!(events.contains(&"native-b:postinstall"));
    for mock in mocks {
        mock.assert();
    }
}

#[test]
fn concurrent_hooks_share_complete_tool_bootstrap_despite_shadowed_ut() {
    bootstrap_case(false);
}

#[test]
fn failed_optional_tool_preparation_can_retry() {
    bootstrap_case(true);
}
