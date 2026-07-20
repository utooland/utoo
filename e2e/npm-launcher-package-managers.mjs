import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const templates = join(repoRoot, "vendor", "templates");
const sandbox = mkdtempSync(join(tmpdir(), "utoo-launcher-pm-"));
const version = "9.9.9-e2e";
const requestedManagers = process.argv.slice(2);
const managers = requestedManagers.length
  ? requestedManagers
  : ["npm", "pnpm", "yarn", "bun"];

process.on("exit", () => {
  try {
    rmSync(sandbox, { recursive: true, force: true });
  } catch {
    // A Windows scanner can briefly retain a copied executable after exit.
  }
});

function executable(command) {
  if (process.platform !== "win32") return command;
  if (
    command.includes("\\") ||
    command.includes("/") ||
    /\.(cmd|exe)$/i.test(command)
  ) {
    return command;
  }
  return command === "bun" ? "bun.exe" : `${command}.cmd`;
}

function run(command, args, options = {}) {
  const resolvedCommand = executable(command);
  const useCommandShell =
    process.platform === "win32" && resolvedCommand.endsWith(".cmd");
  const spawnCommand = useCommandShell
    ? process.env.ComSpec || "cmd.exe"
    : resolvedCommand;
  const spawnArgs = useCommandShell
    ? ["/d", "/s", "/c", "call", resolvedCommand, ...args]
    : args;
  const result = spawnSync(spawnCommand, spawnArgs, {
    cwd: options.cwd,
    env: options.env || process.env,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.error || result.status !== (options.status ?? 0)) {
    const detail = [result.stdout, result.stderr, result.error?.message]
      .filter(Boolean)
      .join("\n");
    throw new Error(
      `${command} ${args.join(" ")} exited ${result.status}\n${detail}`,
    );
  }
  return `${result.stdout || ""}${result.stderr || ""}`;
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function fileSpec(path) {
  return `file:${path.replaceAll("\\", "/")}`;
}

function pack(directory) {
  const tarball = run("npm", ["pack", "--silent"], {
    cwd: directory,
    capture: true,
  }).trim();
  return join(directory, tarball.split(/\r?\n/).at(-1));
}

function target() {
  let arch = process.arch;
  if (process.platform === "win32" && arch === "arm64") arch = "x64";
  const supported = new Set([
    "darwin-x64",
    "darwin-arm64",
    "linux-x64",
    "linux-arm64",
    "win32-x64",
  ]);
  const key = `${process.platform}-${arch}`;
  if (!supported.has(key)) throw new Error(`unsupported test target: ${key}`);
  return {
    arch,
    executable: process.platform === "win32" ? "utoo.exe" : "utoo",
    packageName: `@utoo/utoo-${key}`,
  };
}

function buildPackages() {
  const selected = target();
  const platformDir = join(sandbox, "platform");
  const mainDir = join(sandbox, "main");
  mkdirSync(join(platformDir, "bin"), { recursive: true });
  mkdirSync(join(mainDir, "bin"), { recursive: true });

  const nativePath = join(platformDir, "bin", selected.executable);
  if (process.platform === "win32") {
    // A PE executable is required for the Windows launcher path. Node itself is
    // a convenient native fixture and lets the test validate exact argv/exit
    // forwarding without compiling another program.
    copyFileSync(process.execPath, nativePath);
  } else {
    writeFileSync(
      nativePath,
      [
        "#!/bin/sh",
        'if [ "${1:-}" = "exit-23" ]; then exit 23; fi',
        'printf "ROOT=%s\\n" "${UTOO_MANAGED_PACKAGE_ROOT:-}"',
        'printf "ARGC=%s\\n" "$#"',
        "i=1",
        'for arg in "$@"; do',
        '  printf "ARGV[%s]=%s\\n" "$i" "$arg"',
        "  i=$((i + 1))",
        "done",
        "",
      ].join("\n"),
    );
    chmodSync(nativePath, 0o755);
  }

  writeJson(join(platformDir, "package.json"), {
    name: selected.packageName,
    version,
    preferUnplugged: true,
    os: [process.platform],
    cpu:
      process.platform === "win32"
        ? ["x64", "arm64"]
        : [selected.arch],
  });
  const platformTarball = pack(platformDir);

  copyFileSync(
    join(templates, "launcher.utoo.js.template"),
    join(mainDir, "bin", "launcher.js"),
  );
  copyFileSync(
    join(templates, "utoo.utoo.js.template"),
    join(mainDir, "bin", "utoo.js"),
  );
  copyFileSync(
    join(templates, "utx.utoo.js.template"),
    join(mainDir, "bin", "utx.js"),
  );
  if (process.platform !== "win32") {
    chmodSync(join(mainDir, "bin", "utoo.js"), 0o755);
    chmodSync(join(mainDir, "bin", "utx.js"), 0o755);
  }
  writeJson(join(mainDir, "package.json"), {
    name: "utoo",
    version,
    bin: {
      utoo: "bin/utoo.js",
      ut: "bin/utoo.js",
      utx: "bin/utx.js",
    },
    engines: { node: ">=16" },
    optionalDependencies: {
      [selected.packageName]: fileSpec(platformTarball),
    },
  });

  return pack(mainDir);
}

function installLocal(manager, mainTarball, project) {
  writeJson(join(project, "package.json"), {
    name: `utoo-launcher-${manager}-e2e`,
    private: true,
    packageManager: manager === "yarn" ? "yarn@4.9.2" : undefined,
  });

  const spec = fileSpec(mainTarball);
  if (manager === "npm") {
    run(
      "npm",
      ["install", "--ignore-scripts", "--no-audit", "--no-fund", spec],
      { cwd: project },
    );
  } else if (manager === "pnpm") {
    run("pnpm", ["add", "--ignore-scripts", `utoo@${spec}`], {
      cwd: project,
    });
  } else if (manager === "yarn") {
    const yarnVersion = run("yarn", ["--version"], {
      cwd: project,
      capture: true,
    }).trim();
    if (Number.parseInt(yarnVersion, 10) >= 2) {
      writeFileSync(
        join(project, ".yarnrc.yml"),
        "nodeLinker: pnp\nenableScripts: false\n",
      );
      run("yarn", ["add", `utoo@${spec}`], { cwd: project });
    } else {
      run("yarn", ["add", "--ignore-scripts", spec], { cwd: project });
    }
  } else if (manager === "bun") {
    run("bun", ["add", "--ignore-scripts", `utoo@${spec}`], {
      cwd: project,
    });
  } else {
    throw new Error(`unknown package manager: ${manager}`);
  }
}

function localCommand(manager, project, name, args, options = {}) {
  if (manager === "yarn" && !existsSync(join(project, "node_modules"))) {
    return run("yarn", [name, ...args], {
      cwd: project,
      capture: true,
      status: options.status,
    });
  }
  const suffix = process.platform === "win32" ? ".cmd" : "";
  return run(join(project, "node_modules", ".bin", `${name}${suffix}`), args, {
    cwd: project,
    capture: true,
    status: options.status,
  });
}

function assertIncludes(actual, expected, context) {
  if (!actual.includes(expected)) {
    throw new Error(
      `${context}: expected ${JSON.stringify(expected)} in:\n${actual}`,
    );
  }
}

function verifyCommands(manager, project) {
  if (process.platform === "win32") {
    const printArgs =
      "console.log('ROOT=' + (process.env.UTOO_MANAGED_PACKAGE_ROOT || ''));" +
      "console.log(JSON.stringify(process.argv.slice(1)))";
    const output = localCommand(manager, project, "utoo", [
      "-e",
      printArgs,
      "space arg",
      "你好",
    ]);
    if (!/ROOT=.+/.test(output)) {
      throw new Error(`${manager} managed root was empty:\n${output}`);
    }
    assertIncludes(output, '["space arg","你好"]', `${manager} argv`);
    localCommand(manager, project, "utoo", ["-e", "process.exit(23)"], {
      status: 23,
    });
    assertIncludes(
      localCommand(manager, project, "ut", ["--version"]),
      process.version,
      `${manager} ut alias`,
    );
    writeFileSync(
      join(project, "x"),
      "console.log(JSON.stringify(process.argv.slice(2)))\n",
    );
    assertIncludes(
      localCommand(manager, project, "utx", ["space arg", "你好"]),
      '["space arg","你好"]',
      `${manager} utx`,
    );
  } else {
    const output = localCommand(manager, project, "utoo", [
      "hello",
      "space arg",
      "你好",
    ]);
    if (!/ROOT=.+/.test(output)) {
      throw new Error(`${manager} managed root was empty:\n${output}`);
    }
    assertIncludes(output, "ARGC=3", `${manager} argc`);
    assertIncludes(output, "ARGV[2]=space arg", `${manager} spaced argv`);
    assertIncludes(output, "ARGV[3]=你好", `${manager} Unicode argv`);
    localCommand(manager, project, "utoo", ["exit-23"], { status: 23 });
    assertIncludes(
      localCommand(manager, project, "ut", ["alias"]),
      "ARGV[1]=alias",
      `${manager} ut alias`,
    );
    const utx = localCommand(manager, project, "utx", ["space arg", "你好"]);
    assertIncludes(utx, "ARGV[1]=x", `${manager} utx command`);
    assertIncludes(utx, "ARGV[3]=你好", `${manager} utx argv`);
  }
}

function verifyManifest(manager, project) {
  if (manager === "yarn" && !existsSync(join(project, "node_modules"))) {
    const script =
      "const p=require('utoo/package.json');process.exit(p.scripts ? 1 : 0)";
    run("yarn", ["node", "-e", script], { cwd: project });
    return;
  }
  const manifest = JSON.parse(
    readFileSync(join(project, "node_modules", "utoo", "package.json")),
  );
  if (manifest.scripts) throw new Error(`${manager} installed lifecycle scripts`);
}

function verifyGlobal(manager, mainTarball) {
  if (manager !== "npm" && manager !== "pnpm") return;
  const prefix = join(sandbox, `${manager}-global`);
  mkdirSync(prefix, { recursive: true });
  const spec = fileSpec(mainTarball);
  let binDir;

  if (manager === "npm") {
    run("npm", [
      "install",
      "--global",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      "--prefix",
      prefix,
      spec,
    ]);
    binDir = process.platform === "win32" ? prefix : join(prefix, "bin");
  } else {
    binDir = join(prefix, "bin");
    mkdirSync(binDir, { recursive: true });
    run(
      "pnpm",
      [
        "add",
        "--global",
        "--ignore-scripts",
        "--global-dir",
        join(prefix, "packages"),
        spec,
      ],
      {
        env: {
          ...process.env,
          PATH: `${binDir}${delimiter}${process.env.PATH || ""}`,
          PNPM_HOME: binDir,
        },
      },
    );
  }

  const project = join(sandbox, `${manager}-global-cwd`);
  mkdirSync(project, { recursive: true });
  const suffix = process.platform === "win32" ? ".cmd" : "";
  const output = run(
    join(binDir, `utoo${suffix}`),
    process.platform === "win32" ? ["--version"] : ["global"],
    { cwd: project, capture: true },
  );
  assertIncludes(
    output,
    process.platform === "win32" ? process.version : "ARGV[1]=global",
    `${manager} global launcher`,
  );
}

const mainTarball = buildPackages();
for (const manager of managers) {
  const project = join(sandbox, `${manager}-local`);
  mkdirSync(project, { recursive: true });
  process.stdout.write(`\n== ${manager} (${process.platform}-${process.arch}) ==\n`);
  installLocal(manager, mainTarball, project);
  verifyCommands(manager, project);
  verifyManifest(manager, project);
  verifyGlobal(manager, mainTarball);
  process.stdout.write(`PASS ${manager}\n`);
}
