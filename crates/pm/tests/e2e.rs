use serde_json::Value;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const REGISTRY: &str = "https://registry.npmjs.org";

#[test]
fn pm_e2e_flow() {
    if !env_flag("UTOO_RUN_PM_E2E") {
        eprintln!("skipping PM e2e flow; set UTOO_RUN_PM_E2E=1 to run it");
        return;
    }

    let env = TestEnv::new();

    println!("utoo: {}", env.utoo.display());
    println!("ut: {}", env.ut.display());

    env.output_ok(env.utoo(["--version"]), "utoo --version");
    env.output_ok(env.ut(["--version"]), "ut --version");

    case_large_repos(&env);
    case_basic_fixture_install_link_deps(&env);
    case_global_install_coexistence(&env);
    case_dependency_protocols(&env);
    case_cross_device_cache(&env);
    case_catalog_and_pack_protocols(&env);
    case_npm_aliases(&env);
    case_platform_optional_dependencies(&env);
    case_npm_pack_global_prefix(&env);
    case_link_prefix(&env);
    case_workspace_behaviors(&env);
    case_pnpm_migration(&env);
    case_install_node_esbuild(&env);
    case_broken_pipe_and_script_exit(&env);
    case_peer_deps_config(&env);
    case_permission_normalization(&env);
    case_add_aliases(&env);
    case_dev_prod_dedup_lockfile(&env);
}

struct TestEnv {
    _tmp: TempDir,
    root: PathBuf,
    path_env: OsString,
    home: PathBuf,
    config_home: PathBuf,
    data_home: PathBuf,
    appdata: PathBuf,
    bin_dir: PathBuf,
    utoo: PathBuf,
    ut: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create e2e tempdir");
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
            fs::create_dir_all(dir).expect("create env dir");
        }

        let source_bin = env::var_os("UTOO_E2E_BIN")
            .map(PathBuf::from)
            .or_else(|| option_env!("CARGO_BIN_EXE_utoo").map(PathBuf::from))
            .expect("UTOO_E2E_BIN or CARGO_BIN_EXE_utoo must be available");
        assert!(
            source_bin.exists(),
            "utoo binary does not exist: {}",
            source_bin.display()
        );

        let exe = env::consts::EXE_SUFFIX;
        let utoo = bin_dir.join(format!("utoo{exe}"));
        let ut = bin_dir.join(format!("ut{exe}"));
        fs::copy(&source_bin, &utoo).expect("copy utoo test binary");
        fs::copy(&source_bin, &ut).expect("copy ut test binary");
        make_executable(&utoo);
        make_executable(&ut);

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
            bin_dir,
            utoo,
            ut,
        }
    }

    fn temp_path(&self, name: &str) -> PathBuf {
        let path = self._tmp.path().join(name);
        fs::create_dir_all(&path).expect("create temp case dir");
        path
    }

    fn fixture(&self, relative: &str) -> PathBuf {
        let src = self.root.join("e2e/pm").join(relative);
        let dst = self
            ._tmp
            .path()
            .join("fixtures")
            .join(relative.replace(['/', '\\'], "__"));
        if dst.exists() {
            fs::remove_dir_all(&dst).expect("remove stale copied fixture");
        }
        copy_dir_filtered(&src, &dst)
            .unwrap_or_else(|e| panic!("copy fixture {} -> {}: {e}", src.display(), dst.display()));
        dst
    }

    fn command<P: AsRef<Path>>(&self, program: P) -> Command {
        let program = program.as_ref();
        // On Windows only native `.exe` images launch via CreateProcess; a
        // `.cmd`/`.bat`/extensionless shim (npm-style bin, utoo's `.bin` shim,
        // an npm-installed `utoo.cmd`) must go through cmd.exe. utoo/ut are real
        // `.exe` binaries and launch directly.
        let mut cmd = if cfg!(windows) && !is_windows_exe(program) {
            let mut cmd = Command::new("cmd");
            cmd.arg("/c").arg(program);
            cmd
        } else {
            Command::new(program)
        };
        self.configure(&mut cmd);
        cmd
    }

    fn named_command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
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

    fn utoo<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command(&self.utoo);
        cmd.args(args);
        cmd
    }

    fn ut<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command(&self.ut);
        cmd.args(args);
        cmd
    }

    fn npm<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        // `npm` resolves to `npm.cmd` on Windows, which can't be launched via
        // CreateProcess directly; route it through cmd.exe.
        let mut cmd = if cfg!(windows) {
            let mut cmd = self.named_command("cmd");
            cmd.arg("/c").arg("npm");
            cmd
        } else {
            self.named_command("npm")
        };
        cmd.args(args);
        cmd
    }

    fn node<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.named_command("node");
        cmd.args(args);
        cmd
    }

    fn git<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.named_command("git");
        cmd.args(args);
        cmd
    }

    fn output_ok(&self, mut cmd: Command, context: &str) -> Output {
        let debug = format!("{cmd:?}");
        let output = cmd
            .output()
            .unwrap_or_else(|e| panic!("{context}: failed to spawn {debug}: {e}"));
        assert_success(context, &debug, &output);
        output
    }

    fn output_fail(&self, mut cmd: Command, context: &str) -> Output {
        let debug = format!("{cmd:?}");
        let output = cmd
            .output()
            .unwrap_or_else(|e| panic!("{context}: failed to spawn {debug}: {e}"));
        assert!(
            !output.status.success(),
            "{context}: expected failure for {debug}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        output
    }
}

fn case_large_repos(env: &TestEnv) {
    println!("case: ant-design-x and ant-design installs");
    let antx = env.temp_path("ant-design-x");
    env.output_ok(
        env.git([
            "clone",
            "--branch",
            "next",
            "--single-branch",
            "--depth",
            "1",
            "https://github.com/ant-design/x.git",
            antx.to_str().expect("utf8 path"),
        ]),
        "clone ant-design-x",
    );
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&antx);
    env.output_ok(install, "install ant-design-x");
    let mut rebuild = env.utoo(["rebuild"]);
    rebuild.current_dir(&antx);
    env.output_ok(rebuild, "rebuild ant-design-x");

    let antd = env.temp_path("ant-design");
    env.output_ok(
        env.git([
            "clone",
            "--depth",
            "1",
            "--single-branch",
            "https://github.com/ant-design/ant-design.git",
            antd.to_str().expect("utf8 path"),
        ]),
        "clone ant-design",
    );
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&antd);
    env.output_ok(install, "install ant-design");

    let mut reinstall = env.utoo(["install", "--registry", REGISTRY]);
    reinstall.current_dir(&antd);
    env.output_ok(reinstall, "reinstall ant-design with npm registry");
}

fn case_basic_fixture_install_link_deps(env: &TestEnv) {
    println!("case: basic fixture install/link/deps");
    let antd = env.fixture("antd-test");
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&antd);
    env.output_ok(install, "antd-test install");
    assert_dir(antd.join("node_modules"));
    assert_dir(antd.join("node_modules/antd"));

    let local = env.fixture("local-package");
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&local);
    env.output_ok(install, "local-package install");
    let mut link = env.utoo(["link"]);
    link.current_dir(&local);
    env.output_ok(link, "local-package link");

    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&antd);
    env.output_ok(install, "antd-test warm install");
    assert_dir(antd.join("node_modules/lodash"));

    let mut deps = env.utoo(["deps"]);
    deps.current_dir(&antd);
    env.output_ok(deps, "antd-test deps");
    assert_file(antd.join("package-lock.json"));
    let lock = read_to_string(antd.join("package-lock.json"));
    assert_contains(&lock, "antd", "antd-test lockfile");
    assert_contains(&lock, "react", "antd-test lockfile");
}

fn case_global_install_coexistence(env: &TestEnv) {
    println!("case: global install coexistence");
    env.output_ok(
        env.utoo(["install", "-g", "cowsay", "--registry", REGISTRY]),
        "global install cowsay",
    );
    assert_global_bin(&env.bin_dir, "cowsay");

    env.output_ok(
        env.utoo(["install", "-g", "semver", "--registry", REGISTRY]),
        "global install semver",
    );
    assert_global_bin(&env.bin_dir, "semver");
    assert_global_bin(&env.bin_dir, "cowsay");
}

fn case_dependency_protocols(env: &TestEnv) {
    println!("case: git/http/file deps and stale lockfile");
    let git_deps = env.fixture("git-deps");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&git_deps);
    env.output_ok(install, "git deps install");
    for pkg in ["abbrev", "ini", "isexe"] {
        assert_dir(git_deps.join("node_modules").join(pkg));
    }
    let mut warm = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    warm.current_dir(&git_deps);
    env.output_ok(warm, "git deps warm install");

    let http = env.fixture("http-tarball-deps");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&http);
    env.output_ok(install, "http tarball deps install");
    for pkg in ["abbrev", "ini", "isexe"] {
        assert_file(http.join("node_modules").join(pkg).join("package.json"));
    }
    let lock = read_json(http.join("package-lock.json"));
    assert!(
        lock.to_string()
            .contains("https://registry.npmjs.org/abbrev/-/abbrev-2.0.0.tgz"),
        "tarball resolved URL missing from lockfile"
    );
    let mut warm = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    warm.current_dir(&http);
    env.output_ok(warm, "http tarball deps warm install");

    let file_deps = env.fixture("file-deps");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&file_deps);
    env.output_ok(install, "file deps install");
    assert_file(file_deps.join("node_modules/local-dir-pkg/package.json"));
    assert_file(file_deps.join("node_modules/local-tarball-pkg/package.json"));
    assert_symlink_target(
        &file_deps.join("node_modules/local-dir-pkg"),
        Path::new("../local-dir"),
    );
    assert!(
        !is_symlink(&file_deps.join("node_modules/local-tarball-pkg")),
        "local-tarball-pkg should be a real directory"
    );
    assert_package_name_version(
        &file_deps.join("node_modules/local-dir-pkg/package.json"),
        "local-dir-pkg",
        "0.1.0",
    );
    assert_package_name_version(
        &file_deps.join("node_modules/local-tarball-pkg/package.json"),
        "local-tarball-pkg",
        "2.3.4",
    );
    let lock = read_to_string(file_deps.join("package-lock.json"));
    assert_contains(&lock, "file:./local-dir", "file deps lockfile");
    assert_contains(&lock, "file:./local-tarball.tgz", "file deps lockfile");
    let mut warm = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    warm.current_dir(&file_deps);
    env.output_ok(warm, "file deps warm install");
    assert_symlink_target(
        &file_deps.join("node_modules/local-dir-pkg"),
        Path::new("../local-dir"),
    );

    let stale = env.fixture("stale-lockfile");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&stale);
    env.output_ok(install, "stale lockfile install");
    for pkg in ["abbrev", "ini"] {
        assert_dir(stale.join("node_modules").join(pkg));
    }
    let lock = read_to_string(stale.join("package-lock.json"));
    assert_contains(&lock, "abbrev", "stale lockfile regenerated deps");
    assert_contains(&lock, "ini", "stale lockfile regenerated deps");
}

fn case_cross_device_cache(env: &TestEnv) {
    println!("case: cross-device cache handling");
    #[cfg(not(target_os = "linux"))]
    {
        let _ = env;
        eprintln!("skipping cross-device cache case on non-Linux");
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;

        let shm_root = Path::new("/dev/shm");
        if !shm_root.is_dir() {
            eprintln!("skipping cross-device cache case: /dev/shm unavailable");
            return;
        }

        let disk_project = env.temp_path("xdev-disk-project");
        let shm_parent = tempfile::tempdir_in(shm_root).expect("create /dev/shm tempdir");
        let shm_project = shm_parent.path().join("project");
        fs::create_dir_all(&shm_project).expect("create shm project");

        let disk_dev = fs::metadata(&disk_project).expect("disk metadata").dev();
        let shm_dev = fs::metadata(&shm_project).expect("shm metadata").dev();
        if disk_dev == shm_dev {
            eprintln!("skipping cross-device cache case: temp dir and /dev/shm share device");
            return;
        }

        write_file(
            disk_project.join("package.json"),
            r#"{"name":"xdev-explicit","version":"1.0.0","dependencies":{"is-odd":"3.0.1"}}"#,
        );
        let explicit_cache = shm_parent.path().join("cache");
        let mut explicit = env.utoo(["install", "--registry", REGISTRY]);
        explicit
            .env("UTOO_CACHE_DIR", &explicit_cache)
            .current_dir(&disk_project);
        env.output_ok(explicit, "explicit cross-device cache install");
        assert_dir(disk_project.join("node_modules/is-odd"));
        assert_dir(explicit_cache);

        write_file(
            shm_project.join("package.json"),
            r#"{"name":"xdev-default","version":"1.0.0","dependencies":{"is-odd":"3.0.1"}}"#,
        );
        let mut default_cache = env.utoo(["install", "--registry", REGISTRY]);
        default_cache
            .env_remove("UTOO_CACHE_DIR")
            .current_dir(&shm_project);
        env.output_ok(default_cache, "default cross-device cache install");
        assert_dir(shm_project.join("node_modules/is-odd"));
        assert_dir(shm_project.join("node_modules/.cache/nm"));
    }
}

fn case_catalog_and_pack_protocols(env: &TestEnv) {
    println!("case: catalog protocol and pm-pack rewrites");
    let catalog = env.fixture("catalog-test");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&catalog);
    env.output_ok(install, "catalog install");
    assert_dir(catalog.join("node_modules/lodash"));
    assert_dir(catalog.join("node_modules/typescript"));
    let lock = read_to_string(catalog.join("package-lock.json"));
    assert_contains(&lock, "lodash", "catalog lockfile");
    assert!(
        !lock.contains("catalog:"),
        "catalog specs should be resolved in lockfile"
    );

    let config = catalog.join(".utoo.toml");
    let original = read_to_string(&config);
    let updated = original
        .replace("lodash = \"^4.17.21\"", "lodash = \"^4.17.0\"")
        .replace("debug = \"^4.3.4\"", "debug = \"^3.2.7\"");
    fs::write(&config, updated).expect("update catalog config");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&catalog);
    env.output_ok(install, "catalog updated install");
    let lock = read_to_string(catalog.join("package-lock.json"));
    assert_contains(&lock, "^4.17.0", "catalog updated lockfile");
    assert_contains(&lock, "^3.2.7", "catalog named updated lockfile");
    fs::write(&config, original).expect("restore catalog config");

    let pack = env.fixture("pack-protocols");
    let package_dir = pack.join("packages/foo");
    let before = read_to_string(package_dir.join("package.json"));
    let mut cmd = env.utoo(["pm-pack"]);
    cmd.current_dir(&package_dir);
    env.output_ok(cmd, "pm-pack protocol rewrite");
    let tarball = first_matching_file(&package_dir, "pack-protocols-foo-", ".tgz");
    let inspect = env.temp_path("pack-inspect");
    let mut tar = env.named_command("tar");
    tar.args(["-xzf"]).arg(&tarball).arg("-C").arg(&inspect);
    env.output_ok(tar, "extract pm-pack tarball");
    let packed_pkg = read_to_string(inspect.join("package/package.json"));
    assert_contains(
        &packed_pkg,
        "\"@pack-protocols/bar\": \"^2.4.1\"",
        "packed manifest",
    );
    assert_contains(
        &packed_pkg,
        "\"@pack-protocols/bar\": \"~2.4.1\"",
        "packed manifest",
    );
    assert_contains(
        &packed_pkg,
        "\"@pack-protocols/bar\": \"2.4.1\"",
        "packed manifest",
    );
    assert_contains(&packed_pkg, "\"lodash\": \"^4.17.21\"", "packed manifest");
    assert!(
        !packed_pkg.contains("workspace:") && !packed_pkg.contains("catalog:"),
        "packed manifest should not contain raw workspace/catalog protocols"
    );
    assert_eq!(read_to_string(package_dir.join("package.json")), before);
}

fn case_npm_aliases(env: &TestEnv) {
    println!("case: npm alias install");
    let dir = env.fixture("npm-alias");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "npm alias install");
    assert_package_name(&dir.join("node_modules/my-jquery/package.json"), "jquery");
    assert_package_name(
        &dir.join("node_modules/my-types/package.json"),
        "@types/node",
    );
    assert_package_name(
        &dir.join("node_modules/string-width-cjs/package.json"),
        "string-width",
    );
    assert_dir(dir.join("node_modules/strip-ansi"));
    assert_package_name(
        &dir.join("node_modules/undici-types/package.json"),
        "lodash",
    );
    assert_package_name(
        &dir.join("node_modules/my-types/node_modules/undici-types/package.json"),
        "undici-types",
    );
    assert_package_name(&dir.join("node_modules/ms/package.json"), "raw-body");
    assert!(
        !dir.join("node_modules/debug/node_modules/ms").exists(),
        "debug should reuse top-level aliased ms"
    );
    assert_package_name(
        &dir.join("node_modules/@myorg/utils/package.json"),
        "lodash",
    );
    assert_package_name(
        &dir.join("node_modules/@myorg/types/package.json"),
        "@types/node",
    );
}

fn case_platform_optional_dependencies(env: &TestEnv) {
    println!("case: platform optional dependency binding");
    let dir = env.temp_path("optional-deps");
    write_file(
        dir.join("package.json"),
        r#"{
  "name": "test-optional-deps",
  "version": "1.0.0",
  "dependencies": {
    "rolldown": "1.0.0-beta.57"
  }
}
"#,
    );
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "rolldown install");
    let binding = rolldown_binding(env);
    assert_dir(dir.join("node_modules").join(binding));
    let mut node = env.node(["-e", "require('rolldown')"]);
    node.current_dir(&dir);
    env.output_ok(node, "load rolldown");
}

fn case_npm_pack_global_prefix(env: &TestEnv) {
    println!("case: npm pack + global prefix inference");
    let pack_dir = env.temp_path("npm-pack");
    let pkg = pack_dir.join("pkg");
    fs::create_dir_all(pkg.join("bin")).expect("create npm package");
    // Keep the platform exe suffix on the physical file (`bin/utoo.exe` on
    // Windows; npm resolves the `bin/utoo` mapping to it), mirroring the old
    // PowerShell e2e so npm's generated shim targets a real executable image.
    let packed_bin = pkg.join(format!("bin/utoo{}", env::consts::EXE_SUFFIX));
    fs::copy(&env.utoo, &packed_bin).expect("copy utoo into npm package");
    make_executable(&packed_bin);
    write_file(
        pkg.join("package.json"),
        r#"{
  "name": "utoo",
  "version": "0.0.0-e2e-test",
  "bin": { "utoo": "bin/utoo", "ut": "bin/utoo" },
  "scripts": { "postinstall": "echo postinstall-ok" }
}
"#,
    );
    let mut pack = env.npm(["pack"]);
    pack.current_dir(&pkg);
    env.output_ok(pack, "npm pack utoo");
    let tarball = first_matching_file(&pkg, "utoo-", ".tgz");

    let prefix = env.temp_path("npm-prefix");
    let mut install = env.npm(["install", "-g"]);
    install.arg(&tarball).arg("--prefix").arg(&prefix);
    env.output_ok(install, "npm install -g packed utoo");
    let installed = installed_utoo_path(&prefix);
    assert_exists(&installed);
    let mut version = env.command(&installed);
    version.arg("--version");
    env.output_ok(version, "npm-installed utoo --version");

    let mut global = env.command(&installed);
    global.args(["install", "-g", "cowsay", "--registry", REGISTRY]);
    env.output_ok(global, "global install through npm-installed utoo");
    assert_global_bin(&prefix.join(bin_dir_name()), "cowsay");
    assert_dir(prefix.join(global_node_modules()).join("cowsay"));
    assert!(
        global_bin_path(
            &prefix.join(global_node_modules()).join("utoo").join("bin"),
            "cowsay",
        )
        .is_none(),
        "cowsay bin leaked into utoo package"
    );

    let env_prefix = env.temp_path("env-prefix");
    let mut semver = env.command(&installed);
    semver.env("UTOO_PREFIX", &env_prefix).args([
        "install",
        "-g",
        "semver",
        "--registry",
        REGISTRY,
    ]);
    env.output_ok(semver, "UTOO_PREFIX global install");
    assert_global_bin(&env_prefix.join(bin_dir_name()), "semver");
    assert_dir(env_prefix.join(global_node_modules()).join("semver"));
}

fn case_link_prefix(env: &TestEnv) {
    println!("case: link prefix bins");
    let dir = env.fixture("link-with-bin");
    let prefix = env.temp_path("link-prefix").canonicalize().expect("prefix");
    let mut link = env.utoo(["link", "--prefix"]);
    link.arg(&prefix).current_dir(&dir);
    env.output_ok(link, "utoo link --prefix");
    assert_global_bin(&prefix.join(bin_dir_name()), "link-bin-test");
    assert_exists(prefix.join(global_node_modules()).join("link-bin-test"));
}

fn case_workspace_behaviors(env: &TestEnv) {
    println!("case: workspace topology, hooks, anonymous packages, run output");
    let cycle = env.fixture("workspace-cycle");
    let mut deps = env.ut(["deps", "--workspace-only"]);
    deps.current_dir(&cycle);
    env.output_ok(deps, "workspace cycle deps");
    let workspace = read_json(cycle.join("workspace.json"));
    let topology = workspace["topology"].as_array().expect("topology array");
    assert!(topology.len() >= 2, "expected at least 2 topology layers");
    assert!(
        topology[0].to_string().contains("lib-b"),
        "lib-b should be in first topology layer: {}",
        topology[0]
    );

    let prepare = env.fixture("workspace-prepare");
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&prepare);
    env.output_ok(install, "workspace prepare install");
    assert_file(prepare.join("lib-a/lib/index.js"));
    assert_file(prepare.join("lib-b/lib/index.js"));
    assert_line_count(prepare.join("lib-a/.markers/postinstall"), 1);
    assert_exists(
        prepare
            .join("node_modules/.bin")
            .join(bin_name("lib-a-cli")),
    );

    fs::remove_dir_all(prepare.join("lib-a/lib")).ok();
    fs::remove_dir_all(prepare.join("lib-a/.markers")).ok();
    fs::remove_dir_all(prepare.join("lib-b/lib")).ok();
    let mut rebuild = env.utoo(["rebuild"]);
    rebuild.current_dir(&prepare);
    env.output_ok(rebuild, "workspace prepare rebuild");
    assert_file(prepare.join("lib-a/lib/index.js"));
    assert_file(prepare.join("lib-b/lib/index.js"));
    assert_line_count(prepare.join("lib-a/.markers/postinstall"), 1);

    fs::remove_dir_all(prepare.join("node_modules")).ok();
    fs::remove_dir_all(prepare.join("lib-a/lib")).ok();
    fs::remove_dir_all(prepare.join("lib-a/.markers")).ok();
    fs::remove_dir_all(prepare.join("lib-b/lib")).ok();
    fs::remove_file(prepare.join("package-lock.json")).ok();
    let mut ignored = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    ignored.current_dir(&prepare);
    env.output_ok(ignored, "workspace prepare ignore scripts");
    assert!(
        !prepare.join("lib-a/lib/index.js").exists()
            && !prepare.join("lib-a/.markers/postinstall").exists(),
        "--ignore-scripts should skip workspace hooks"
    );

    let anonymous = env.fixture("workspace-anonymous");
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&anonymous);
    env.output_ok(install, "anonymous workspace install");
    assert_file(anonymous.join("anon-a/marker-postinstall"));
    assert_file(anonymous.join("anon-b/marker-prepare"));

    let run_ws = env.fixture("run-workspaces");
    let mut all = env.ut(["run", "build", "--workspaces"]);
    all.current_dir(&run_ws);
    let out = env.output_ok(all, "ut run build --workspaces");
    let stdout = combined_output(&out);
    assert_contains(
        &stdout,
        "Running build in 3 workspaces, 3 layers",
        "run all",
    );
    assert_contains(&stdout, "1:", "run all");
    assert_contains(&stdout, "lib-b", "run all");
    assert_contains(&stdout, "[lib-b] echo building lib-b", "run all");
    assert_order(
        &stdout,
        "building lib-b",
        "building app",
        "run all topology",
    );

    let mut subset = env.ut(["run", "build", "--workspace", "lib-b", "--workspace", "app"]);
    subset.current_dir(&run_ws);
    let out = env.output_ok(subset, "ut run build subset");
    let stdout = combined_output(&out);
    assert_contains(&stdout, "Running build in 2 workspaces", "run subset");
    assert!(
        !stdout.contains("[lib-a]"),
        "lib-a should be excluded from subset run"
    );
    assert_order(
        &stdout,
        "building lib-b",
        "building app",
        "run subset topology",
    );

    let mut glob = env.ut(["run", "build", "--workspace", "lib-*"]);
    glob.current_dir(&run_ws);
    let out = env.output_ok(glob, "ut run build glob");
    let stdout = combined_output(&out);
    assert_contains(&stdout, "Running build in 2 workspaces", "run glob");
    assert_contains(&stdout, "[lib-a]", "run glob");
    assert_contains(&stdout, "[lib-b]", "run glob");
    assert!(!stdout.contains("[app]"), "glob should not match app");

    let mut if_present = env.ut(["run", "test", "--workspaces", "--if-present"]);
    if_present.current_dir(&run_ws);
    let out = env.output_ok(if_present, "ut run test --if-present");
    let stdout = combined_output(&out);
    assert_contains(&stdout, "[lib-b] echo testing lib-b", "run if-present");
    assert!(
        !stdout.contains("[lib-a]") && !stdout.contains("[app]"),
        "--if-present should not announce missing scripts"
    );
    assert!(
        !stdout.lines().any(|line| line.trim() == "✓")
            && !stdout.contains("▶ 2/3")
            && !stdout.contains("▶ 3/3"),
        "--if-present printed empty rows/layers:\n{stdout}"
    );
}

fn case_pnpm_migration(env: &TestEnv) {
    println!("case: pnpm migration");
    let egg = env.temp_path("egg");
    env.output_ok(
        env.git([
            "clone",
            "--branch",
            "next",
            "--single-branch",
            "--depth",
            "1",
            "https://github.com/eggjs/egg.git",
            egg.to_str().expect("utf8 path"),
        ]),
        "clone eggjs/egg",
    );
    let mut install = env.utoo([
        "install",
        "--from",
        "pnpm",
        "--ignore-scripts",
        "--registry",
        REGISTRY,
    ]);
    install.current_dir(&egg);
    env.output_ok(install, "pnpm migration install");
    let pkg = read_json(egg.join("package.json"));
    assert!(
        pkg["workspaces"]
            .as_array()
            .is_some_and(|ws| ws.iter().any(|v| v == "packages/*")),
        "workspaces should include packages/*"
    );
    assert!(
        pkg["overrides"].get("vite").is_some(),
        "vite override should be present"
    );
    let config = read_to_string(egg.join(".utoo.toml"));
    assert_contains(&config, "lodash", ".utoo.toml");
    assert_contains(&config, "path-to-regexp", ".utoo.toml");
    assert_dir(egg.join("node_modules"));
}

fn case_install_node_esbuild(env: &TestEnv) {
    println!("case: install-node + esbuild");
    let dir = env.temp_path("install-node-esbuild");
    write_file(
        dir.join("package.json"),
        r#"{
  "name": "install-node-esbuild-test",
  "dependencies": {
    "esbuild": "0.27.0"
  },
  "engines": {
    "install-node": "20"
  }
}
"#,
    );
    let mut install = env.utoo(["install", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "install-node esbuild install");
    let mut node = env.command(dir.join("node_modules/.bin").join(bin_name("node")));
    node.arg("-v");
    env.output_ok(node, "local node -v");
    let mut esbuild = env.command(dir.join("node_modules/.bin").join(bin_name("esbuild")));
    esbuild.arg("--version");
    env.output_ok(esbuild, "local esbuild --version");
    let inode_before = file_identity(dir.join("package-lock.json"));
    let mut warm = env.utoo(["install", "--registry", REGISTRY]);
    warm.current_dir(&dir);
    let out = env.output_ok(warm, "install-node esbuild warm install");
    let inode_after = file_identity(dir.join("package-lock.json"));
    assert_eq!(
        inode_before, inode_after,
        "package-lock.json should not be regenerated on warm install"
    );
    assert!(
        !combined_output(&out).contains("package-lock.json is outdated"),
        "warm install should not warn about stale lockfile"
    );
}

fn case_broken_pipe_and_script_exit(env: &TestEnv) {
    println!("case: broken pipe and script exit");
    let dir = env.temp_path("sigpipe");
    write_file(
        dir.join("package.json"),
        r#"{
  "name": "sigpipe-test",
  "version": "1.0.0",
  "scripts": {
    "prepare": "echo line1 && echo line2 && echo line3",
    "fail": "exit 141"
  }
}
"#,
    );

    #[cfg(unix)]
    {
        let mut shell = env.named_command("sh");
        shell
            .arg("-c")
            .arg("ut run prepare 2>/dev/null | head -1 >/dev/null")
            .current_dir(&dir);
        let out = shell.output().expect("run broken pipe shell");
        let code = out.status.code().unwrap_or(141);
        assert!(
            code == 0 || code == 141,
            "broken pipe caused exit {code}\n{}",
            combined_output(&out)
        );
    }

    let mut fail = env.ut(["run", "fail"]);
    fail.current_dir(&dir);
    env.output_fail(fail, "script exit 141 should fail");
}

fn case_peer_deps_config(env: &TestEnv) {
    println!("case: legacy peer deps config");
    let dir = env.fixture("peer-deps");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "peer deps default install");
    assert!(
        !dir.join("node_modules/react").exists(),
        "react should not be auto-installed by default"
    );
    assert_dir(dir.join("node_modules/react-dom"));

    fs::remove_dir_all(dir.join("node_modules")).ok();
    fs::remove_file(dir.join("package-lock.json")).ok();
    write_file(
        dir.join(".utoo.toml"),
        r#"[values]
legacy-peer-deps = "false"
"#,
    );
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "peer deps legacy=false install");
    assert_dir(dir.join("node_modules/react"));
}

fn case_permission_normalization(env: &TestEnv) {
    println!("case: tarball permission normalization");
    let dir = env.temp_path("perm-normalize");
    write_file(
        dir.join("package.json"),
        r#"{
  "name": "perm-normalize-test",
  "version": "1.0.0",
  "dependencies": {
    "google-protobuf": "4.0.2"
  }
}
"#,
    );
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "google-protobuf install");

    #[cfg(unix)]
    {
        for file in files_under(&dir.join("node_modules/google-protobuf")) {
            let mode = fs::metadata(&file)
                .expect("file metadata")
                .permissions()
                .mode();
            assert!(
                mode & 0o004 != 0,
                "file missing other-read bit: {} mode {:o}",
                file.display(),
                mode & 0o777
            );
        }
        let mode = fs::metadata(dir.join("node_modules/google-protobuf/package.json"))
            .expect("package metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644, "google-protobuf package.json mode");
    }
}

fn case_add_aliases(env: &TestEnv) {
    println!("case: add alias variants");
    let dir = env.temp_path("add-alias");
    write_file(
        dir.join("package.json"),
        r#"{
  "name": "test-add-alias",
  "version": "1.0.0",
  "dependencies": {}
}
"#,
    );
    let mut add = env.utoo(["add", "react", "--ignore-scripts", "--registry", REGISTRY]);
    add.current_dir(&dir);
    env.output_ok(add, "utoo add react");
    assert_dir(dir.join("node_modules/react"));

    let mut add_dev = env.utoo([
        "add",
        "lodash",
        "-D",
        "--ignore-scripts",
        "--registry",
        REGISTRY,
    ]);
    add_dev.current_dir(&dir);
    env.output_ok(add_dev, "utoo add -D");
    let pkg = read_to_string(dir.join("package.json"));
    assert_contains(&pkg, "\"devDependencies\"", "add -D package.json");
    assert_contains(&pkg, "\"lodash\"", "add -D package.json");

    let mut ut_add = env.ut(["add", "express", "--ignore-scripts", "--registry", REGISTRY]);
    ut_add.current_dir(&dir);
    env.output_ok(ut_add, "ut add express");
    assert_dir(dir.join("node_modules/express"));

    let mut add_optional = env.utoo([
        "add",
        "debug@4.3.4",
        "-O",
        "--ignore-scripts",
        "--registry",
        REGISTRY,
    ]);
    add_optional.current_dir(&dir);
    env.output_ok(add_optional, "utoo add -O");
    assert_contains(
        &read_to_string(dir.join("package.json")),
        "\"optionalDependencies\"",
        "add -O package.json",
    );

    let mut add_peer = env.utoo([
        "add",
        "typescript@5.0.4",
        "--save-peer",
        "--ignore-scripts",
        "--registry",
        REGISTRY,
    ]);
    add_peer.current_dir(&dir);
    env.output_ok(add_peer, "utoo add --save-peer");
    assert_contains(
        &read_to_string(dir.join("package.json")),
        "\"peerDependencies\"",
        "add --save-peer package.json",
    );

    env.output_ok(env.utoo(["--help"]), "utoo help includes add");
    env.output_ok(env.utoo(["add", "--help"]), "utoo add --help");

    for (program, args, package) in [
        (
            "utoo",
            vec![
                "install",
                "react",
                "--ignore-scripts",
                "--registry",
                REGISTRY,
            ],
            "react",
        ),
        (
            "ut",
            vec!["i", "lodash", "--ignore-scripts", "--registry", REGISTRY],
            "lodash",
        ),
        (
            "utoo",
            vec![
                "add",
                "is-array",
                "is-object",
                "--ignore-scripts",
                "--registry",
                REGISTRY,
            ],
            "is-array",
        ),
        (
            "utoo",
            vec![
                "add",
                "semver@^7.0.0",
                "--ignore-scripts",
                "--registry",
                REGISTRY,
            ],
            "semver",
        ),
    ] {
        fs::remove_dir_all(dir.join("node_modules")).ok();
        fs::remove_file(dir.join("package-lock.json")).ok();
        let mut cmd = if program == "ut" {
            env.ut(args)
        } else {
            env.utoo(args)
        };
        cmd.current_dir(&dir);
        env.output_ok(cmd, "add/install alias variant");
        assert_dir(dir.join("node_modules").join(package));
    }

    env.output_ok(
        env.utoo(["add", "-g", "cowsay", "--registry", REGISTRY]),
        "utoo add -g cowsay",
    );
    assert_global_bin(&env.bin_dir, "cowsay");
}

fn case_dev_prod_dedup_lockfile(env: &TestEnv) {
    println!("case: prod-reachable dev dependency lockfile mark");
    let dir = env.fixture("dev-prod-dedup");
    let mut install = env.utoo(["install", "--ignore-scripts", "--registry", REGISTRY]);
    install.current_dir(&dir);
    env.output_ok(install, "dev-prod-dedup install");
    let lock = read_json(dir.join("package-lock.json"));
    let packages = lock["packages"].as_object().expect("lock packages object");
    let sdk_base = packages
        .get("node_modules/sdk-base")
        .expect("sdk-base in lockfile");
    let p_timeout = packages
        .get("node_modules/p-timeout")
        .expect("p-timeout in lockfile");
    assert!(
        sdk_base.get("dev") != Some(&Value::Bool(true)),
        "sdk-base wrongly marked dev"
    );
    assert!(
        p_timeout.get("dev") != Some(&Value::Bool(true)),
        "p-timeout is prod-reachable but marked dev:true"
    );
}

fn assert_success(context: &str, debug: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{context}: command failed: {debug}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
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
        ) || name.starts_with("run-")
            || name == ".git"
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

fn make_executable(path: &Path) {
    let _ = path;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(path, perms).expect("chmod executable");
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"))
}

/// True when `program` is a native Windows `.exe` image (launchable directly via
/// CreateProcess). `.cmd`/`.bat`/extensionless shims return false and must be run
/// through `cmd.exe`.
fn is_windows_exe(program: &Path) -> bool {
    program
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn bin_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    }
}

fn bin_dir_name() -> &'static str {
    if cfg!(windows) { "" } else { "bin" }
}

fn global_node_modules() -> &'static str {
    if cfg!(windows) {
        "node_modules"
    } else {
        "lib/node_modules"
    }
}

fn installed_utoo_path(prefix: &Path) -> PathBuf {
    let direct = prefix.join(bin_dir_name()).join(bin_name("utoo"));
    if direct.exists() {
        return direct;
    }
    let unix_deep = prefix.join("lib/node_modules/utoo/bin/utoo");
    if unix_deep.exists() {
        return unix_deep;
    }
    prefix.join("node_modules/utoo/bin/utoo.exe")
}

fn assert_exists(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(path.exists(), "expected path to exist: {}", path.display());
}

/// Locate a global bin shim for `name` under `dir`. Global installs/links create
/// the shim via a bare-name symlink (`name`) on every platform — unlike
/// `node_modules/.bin`, which gets a `.cmd` shim on Windows — so the bare name is
/// the expected form; `.cmd`/`.exe` are also accepted in case the global shim
/// convention changes.
fn global_bin_path(dir: &Path, name: &str) -> Option<PathBuf> {
    [
        name.to_string(),
        format!("{name}.cmd"),
        format!("{name}.exe"),
    ]
    .into_iter()
    .map(|candidate| dir.join(candidate))
    .find(|path| path.exists())
}

fn assert_global_bin(dir: &Path, name: &str) {
    assert!(
        global_bin_path(dir, name).is_some(),
        "expected global bin {name} in {}",
        dir.display()
    );
}

fn assert_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(path.is_file(), "expected file: {}", path.display());
}

fn assert_dir(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(path.is_dir(), "expected directory: {}", path.display());
}

fn assert_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        haystack.contains(needle),
        "{context}: expected to contain {needle:?}\n{haystack}"
    );
}

fn assert_order(text: &str, before: &str, after: &str, context: &str) {
    let before_idx = text.find(before);
    let after_idx = text.find(after);
    assert!(
        before_idx.is_some() && after_idx.is_some() && before_idx < after_idx,
        "{context}: expected {before:?} before {after:?}\n{text}"
    );
}

fn assert_line_count(path: impl AsRef<Path>, expected: usize) {
    let path = path.as_ref();
    let content = read_to_string(path);
    let actual = content.lines().count();
    assert_eq!(actual, expected, "line count for {}", path.display());
}

fn assert_package_name(path: &Path, expected_name: &str) {
    let pkg = read_json(path);
    assert_eq!(pkg["name"], expected_name);
}

fn assert_package_name_version(path: &Path, expected_name: &str, expected_version: &str) {
    let pkg = read_json(path);
    assert_eq!(pkg["name"], expected_name);
    assert_eq!(pkg["version"], expected_version);
}

fn assert_symlink_target(path: &Path, expected: &Path) {
    if cfg!(windows) {
        assert_dir(path);
        return;
    }
    assert!(is_symlink(path), "expected symlink: {}", path.display());
    let target = fs::read_link(path).expect("read symlink");
    assert_eq!(target, expected, "symlink target for {}", path.display());
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn read_to_string(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref())
        .unwrap_or_else(|e| panic!("read {}: {e}", path.as_ref().display()))
}

fn write_file(path: impl AsRef<Path>, content: &str) {
    fs::write(path.as_ref(), content)
        .unwrap_or_else(|e| panic!("write {}: {e}", path.as_ref().display()));
}

fn read_json(path: impl AsRef<Path>) -> Value {
    serde_json::from_str(&read_to_string(path.as_ref()))
        .unwrap_or_else(|e| panic!("parse json {}: {e}", path.as_ref().display()))
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn first_matching_file(dir: &Path, prefix: &str, suffix: &str) -> PathBuf {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("no file matching {prefix}*{suffix} in {}", dir.display()))
}

fn rolldown_binding(env: &TestEnv) -> String {
    let output = env.output_ok(
        env.node(["-p", "process.platform + ':' + process.arch"]),
        "detect node platform",
    );
    let platform = String::from_utf8(output.stdout)
        .expect("node platform utf8")
        .trim()
        .to_string();
    match platform.as_str() {
        "darwin:arm64" => "@rolldown/binding-darwin-arm64".to_string(),
        "darwin:x64" => "@rolldown/binding-darwin-x64".to_string(),
        "linux:x64" => "@rolldown/binding-linux-x64-gnu".to_string(),
        "win32:x64" => "@rolldown/binding-win32-x64-msvc".to_string(),
        other => panic!("unsupported rolldown e2e platform: {other}"),
    }
}

#[cfg(unix)]
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    files
}

#[cfg(unix)]
fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap_or_else(|e| panic!("read dir {}: {e}", root.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

#[cfg(unix)]
fn file_identity(path: impl AsRef<Path>) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path.as_ref()).expect("metadata").ino()
}

#[cfg(windows)]
fn file_identity(path: impl AsRef<Path>) -> u64 {
    fs::metadata(path.as_ref())
        .expect("metadata")
        .modified()
        .expect("modified")
        .duration_since(std::time::UNIX_EPOCH)
        .expect("duration")
        .as_nanos() as u64
}
