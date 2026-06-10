use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const REGISTRY: &str = "https://registry.npmjs.org";

#[test]
fn arborist_fixture_e2e() {
    if !env_flag("UTOO_RUN_PM_E2E") {
        eprintln!("skipping arborist e2e; set UTOO_RUN_PM_E2E=1 to run it");
        return;
    }

    let harness = Harness::new();
    let source = harness.root.join("e2e/pm/arborist");
    let filter = env::var("UTOO_E2E_FILTER").ok();
    let run_all = env_flag("UTOO_E2E_ARBORIST_ALL");
    let skips = skip_list();
    let failures = expected_failures();

    if env_flag("UTOO_E2E_LIST") {
        for fixture in top_level_fixtures(&source) {
            let name = fixture.file_name().unwrap().to_string_lossy();
            if let Some(reason) = skips.get(name.as_ref()) {
                println!("{name} (skip: {reason})");
            } else {
                println!("{name}");
            }
        }
        return;
    }

    let mut pass = 0usize;
    let mut fail = Vec::new();
    let mut skip = 0usize;

    for fixture in top_level_fixtures(&source) {
        let name = fixture
            .file_name()
            .expect("fixture name")
            .to_string_lossy()
            .to_string();
        if !matches_filter(&name, filter.as_deref()) {
            continue;
        }
        if !run_all && let Some(reason) = skips.get(name.as_str()) {
            println!("SKIP {name} ({reason})");
            skip += 1;
            continue;
        }

        let dst = harness.copy_fixture(&fixture, &name);
        let expect_failure = failures.contains(name.as_str());
        match run_install_case(&harness, &dst, &name, expect_failure) {
            Ok(()) => pass += 1,
            Err(err) => fail.push(err),
        }
    }

    let conflict = source.join("testing-peer-dep-conflict-chain");
    if conflict.is_dir() && matches_filter("testing-peer-dep-conflict-chain", filter.as_deref()) {
        for sub in top_level_fixtures(&conflict) {
            let subname = sub.file_name().unwrap().to_string_lossy();
            let name = format!("testing-peer-dep-conflict-chain/{subname}");
            let dst = harness.copy_fixture(&sub, &name.replace('/', "__"));
            match run_install_case(&harness, &dst, &name, false) {
                Ok(()) => pass += 1,
                Err(err) => fail.push(err),
            }
        }
    }

    for name in [
        "testing-peer-deps",
        "dedupe-tests",
        "workspaces-simple",
        "sax",
        "once-outdated",
    ] {
        let reinstall_name = format!("reinstall-{name}");
        if !matches_filter(&reinstall_name, filter.as_deref()) {
            continue;
        }
        if !run_all && let Some(reason) = skips.get(name) {
            println!("SKIP {reinstall_name} ({reason})");
            skip += 1;
            continue;
        }
        let src = source.join(name);
        if !src.join("package.json").is_file() {
            println!("SKIP {reinstall_name} (missing package.json)");
            skip += 1;
            continue;
        }
        let dst = harness.copy_fixture(&src, &reinstall_name);
        if let Err(err) = run_install_case(&harness, &dst, name, false)
            .and_then(|_| run_install_case(&harness, &dst, &reinstall_name, false))
        {
            fail.push(err);
        } else {
            pass += 1;
        }
    }

    println!(
        "arborist results: {pass} passed, {} failed, {skip} skipped",
        fail.len()
    );
    assert!(fail.is_empty(), "arborist failures:\n{}", fail.join("\n\n"));
}

struct Harness {
    _tmp: TempDir,
    root: PathBuf,
    path_env: OsString,
    home: PathBuf,
    config_home: PathBuf,
    data_home: PathBuf,
    appdata: PathBuf,
    utoo: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let bin_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        let config_home = tmp.path().join("config");
        let data_home = tmp.path().join("data");
        let appdata = tmp.path().join("appdata");
        for dir in [&bin_dir, &home, &config_home, &data_home, &appdata] {
            fs::create_dir_all(dir).expect("create harness dir");
        }

        let source_bin = env::var_os("UTOO_E2E_BIN")
            .map(PathBuf::from)
            .or_else(|| option_env!("CARGO_BIN_EXE_utoo").map(PathBuf::from))
            .expect("UTOO_E2E_BIN or CARGO_BIN_EXE_utoo must be available");
        let utoo = bin_dir.join(format!("utoo{}", env::consts::EXE_SUFFIX));
        fs::copy(&source_bin, &utoo).expect("copy utoo");
        make_executable(&utoo);

        let old_path = env::var_os("PATH").unwrap_or_default();
        let path_env =
            env::join_paths(std::iter::once(bin_dir.clone()).chain(env::split_paths(&old_path)))
                .expect("join PATH");

        Self {
            _tmp: tmp,
            root,
            path_env,
            home,
            config_home,
            data_home,
            appdata,
            utoo,
        }
    }

    fn copy_fixture(&self, src: &Path, name: &str) -> PathBuf {
        let dst = self._tmp.path().join("fixtures").join(name);
        if dst.exists() {
            fs::remove_dir_all(&dst).expect("remove fixture copy");
        }
        copy_dir_filtered(src, &dst)
            .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", src.display(), dst.display()));
        dst
    }

    fn utoo<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(&self.utoo);
        cmd.args(args);
        self.configure(&mut cmd);
        cmd
    }

    fn configure(&self, cmd: &mut Command) {
        cmd.env("PATH", &self.path_env)
            .env("HOME", &self.home)
            .env("APPDATA", &self.appdata)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("XDG_DATA_HOME", &self.data_home)
            .env("CI", "true")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "off")
            .env("UTOO_REGISTRY", REGISTRY);
    }
}

fn run_install_case(
    harness: &Harness,
    fixture: &Path,
    name: &str,
    expect_failure: bool,
) -> Result<(), String> {
    let mut cmd = harness.utoo(["install", "--registry", REGISTRY]);
    cmd.current_dir(fixture);
    let debug = format!("{cmd:?}");
    let output = cmd
        .output()
        .map_err(|e| format!("{name}: failed to spawn {debug}: {e}"))?;
    let success = output.status.success();

    if expect_failure {
        if success {
            return Err(format!(
                "{name}: expected install failure but command succeeded\n{}",
                combined_output(&output)
            ));
        }
        println!("PASS {name} (correctly failed)");
        return Ok(());
    }

    if !success {
        return Err(format!(
            "{name}: install failed\nstatus: {}\n{}",
            output.status,
            combined_output(&output)
        ));
    }

    if fixture.join("node_modules").is_dir() || fixture.join("package-lock.json").is_file() {
        println!("PASS {name}");
        return Ok(());
    }

    if dependency_count(fixture.join("package.json")) == 0 {
        println!("PASS {name} (zero deps)");
        return Ok(());
    }

    Err(format!(
        "{name}: install succeeded but no node_modules/package-lock.json was created"
    ))
}

fn top_level_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut fixtures = fs::read_dir(root)
        .unwrap_or_else(|e| panic!("read fixture root {}: {e}", root.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("package.json").is_file())
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

fn dependency_count(package_json: PathBuf) -> usize {
    let pkg: Value = serde_json::from_str(
        &fs::read_to_string(&package_json)
            .unwrap_or_else(|e| panic!("read {}: {e}", package_json.display())),
    )
    .unwrap_or_else(|e| panic!("parse {}: {e}", package_json.display()));
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .filter_map(|key| pkg.get(*key).and_then(Value::as_object))
        .map(serde_json::Map::len)
        .sum()
}

fn matches_filter(name: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| name.to_lowercase().contains(&filter.to_lowercase()))
}

fn expected_failures() -> HashSet<&'static str> {
    [
        "testing-peer-deps-unresolvable",
        "prod-dep-missing",
        "prod-dep-enotarget",
        "prod-dep-tgz-missing",
        "prod-dep-allinstall-fail",
        "prod-dep-install-fail",
        "prod-dep-postinstall-fail",
        "prod-dep-preinstall-fail",
        "workspaces-duplicate",
        "platform-specification",
        "fail-install",
        "fail-preinstall",
        "fail-postinstall",
        "fail-allinstall",
        "bad",
    ]
    .into_iter()
    .collect()
}

fn skip_list() -> HashMap<&'static str, &'static str> {
    let mut skips = HashMap::new();
    for name in [
        "link-dep-cycle",
        "link-dep-lifecycle-scripts",
        "external-link-dep",
        "yarn-stuff",
    ] {
        skips.insert(name, "file: target missing from fixture port");
    }
    for name in [
        "link-meta-deps",
        "link-meta-deps-empty",
        "link-dep-has-dep-with-optional-dep",
        "audit-mkdirp",
    ] {
        skips.insert(name, "file: resolver limitation");
    }
    for name in [
        "optional-dep-tgz-missing",
        "optional-metadep-missing",
        "optional-metadep-enotarget",
    ] {
        skips.insert(name, "optional transitive");
    }
    skips.insert("testing-peer-deps-unresolvable", "strict peer deps");
    skips.insert("platform-specification", "platform reject");
    skips.insert("workspaces-duplicate", "workspace duplicate");
    skips.insert("pathological-dep-nesting-cycle", "dep cycle OOM");
    for name in ["audit-linked-package", "testing-missing-tgz"] {
        skips.insert(name, "mock registry only");
    }
    for name in [
        "workspaces-conflicting-dev-deps",
        "ancient-lockfile-invalid",
    ] {
        skips.insert(name, "misc");
    }
    skips
}

fn copy_dir_filtered(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "node_modules" | "package-lock.json" | "workspace.json"
        ) || name == ".git"
        {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&file_name);
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_filtered(&src_path, &dst_path)?;
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn make_executable(path: &Path) {
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(path, perms).expect("chmod executable");
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"))
}
