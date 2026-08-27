import { spawn } from "node:child_process";
import {
  access,
  chmod,
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  readdir,
  readlink,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const templates = join(repoRoot, "vendor", "templates");
const sandbox = await mkdtemp(join(tmpdir(), "utoo-launcher-pm-"));
const version = "9.9.9-e2e";
const requestedManagers = process.argv.slice(2);
const managers = requestedManagers.length
  ? requestedManagers
  : ["npm", "pnpm", "yarn", "bun"];

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

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
  return new Promise((resolve, reject) => {
    const child = spawn(spawnCommand, spawnArgs, {
      cwd: options.cwd,
      env: options.env || process.env,
      stdio: options.capture ? ["ignore", "pipe", "pipe"] : "inherit",
    });
    let stdout = "";
    let stderr = "";

    if (options.capture) {
      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");
      child.stdout.on("data", (chunk) => {
        stdout += chunk;
      });
      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });
    }

    child.once("error", (error) => {
      reject(
        new Error(`${command} ${args.join(" ")} failed: ${error.message}`, {
          cause: error,
        }),
      );
    });
    child.once("close", (status, signal) => {
      if (status !== (options.status ?? 0)) {
        const detail = [stdout, stderr, signal && `signal: ${signal}`]
          .filter(Boolean)
          .join("\n");
        reject(
          new Error(
            `${command} ${args.join(" ")} exited ${status}\n${detail}`,
          ),
        );
        return;
      }
      resolve(`${stdout}${stderr}`);
    });
  });
}

function writeJson(path, value) {
  return writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
}

function fileSpec(path) {
  return `file:${path.replaceAll("\\", "/")}`;
}

async function pack(directory) {
  const tarball = (
    await run("npm", ["pack", "--silent"], {
      cwd: directory,
      capture: true,
    })
  ).trim();
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

async function buildPackages() {
  const selected = target();
  const platformDir = join(sandbox, "platform");
  const mainDir = join(sandbox, "main");
  await Promise.all([
    mkdir(join(platformDir, "bin"), { recursive: true }),
    mkdir(join(mainDir, "bin"), { recursive: true }),
  ]);

  const nativePath = join(platformDir, "bin", selected.executable);
  if (process.platform === "win32") {
    // A PE executable is required for the Windows launcher path. Node itself is
    // a convenient native fixture and lets the test validate exact argv/exit
    // forwarding without compiling another program.
    await copyFile(process.execPath, nativePath);
  } else {
    await writeFile(
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
    await chmod(nativePath, 0o755);
  }

  await writeJson(join(platformDir, "package.json"), {
    name: selected.packageName,
    version,
    preferUnplugged: true,
    os: [process.platform],
    cpu:
      process.platform === "win32"
        ? ["x64", "arm64"]
        : [selected.arch],
  });
  const platformTarball = await pack(platformDir);

  await Promise.all([
    copyFile(
      join(templates, "launcher.utoo.js.template"),
      join(mainDir, "bin", "launcher.js"),
    ),
    copyFile(
      join(templates, "registry.utoo.js.template"),
      join(mainDir, "bin", "registry.js"),
    ),
    copyFile(
      join(templates, "self-heal.utoo.js.template"),
      join(mainDir, "bin", "self-heal.js"),
    ),
    copyFile(
      join(templates, "utoo.utoo.js.template"),
      join(mainDir, "bin", "utoo.js"),
    ),
    copyFile(
      join(templates, "utx.utoo.js.template"),
      join(mainDir, "bin", "utx.js"),
    ),
  ]);
  if (process.platform !== "win32") {
    await Promise.all([
      chmod(join(mainDir, "bin", "utoo.js"), 0o755),
      chmod(join(mainDir, "bin", "utx.js"), 0o755),
    ]);
  }
  await writeJson(join(mainDir, "package.json"), {
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

  return {
    mainTarball: await pack(mainDir),
    platformDir,
    selected,
  };
}

async function installLocal(manager, mainTarball, project) {
  await writeJson(join(project, "package.json"), {
    name: `utoo-launcher-${manager}-e2e`,
    private: true,
    packageManager: manager === "yarn" ? "yarn@4.9.2" : undefined,
  });

  const spec = fileSpec(mainTarball);
  if (manager === "npm") {
    await run(
      "npm",
      ["install", "--ignore-scripts", "--no-audit", "--no-fund", spec],
      { cwd: project },
    );
  } else if (manager === "pnpm") {
    await run("pnpm", ["add", "--ignore-scripts", `utoo@${spec}`], {
      cwd: project,
    });
  } else if (manager === "yarn") {
    const yarnVersion = (
      await run("yarn", ["--version"], {
        cwd: project,
        capture: true,
      })
    ).trim();
    if (Number.parseInt(yarnVersion, 10) >= 2) {
      await writeFile(
        join(project, ".yarnrc.yml"),
        "nodeLinker: pnp\nenableScripts: false\n",
      );
      await run("yarn", ["add", `utoo@${spec}`], { cwd: project });
    } else {
      await run("yarn", ["add", "--ignore-scripts", spec], { cwd: project });
    }
  } else if (manager === "bun") {
    await run("bun", ["add", "--ignore-scripts", `utoo@${spec}`], {
      cwd: project,
    });
  } else {
    throw new Error(`unknown package manager: ${manager}`);
  }
}

async function localCommand(manager, project, name, args, options = {}) {
  if (manager === "yarn" && !(await exists(join(project, "node_modules")))) {
    return run("yarn", [name, ...args], {
      cwd: project,
      capture: true,
      status: options.status,
    });
  }
  // Bun owns command resolution for its installed package bins. In particular,
  // its Windows layout does not expose npm-style `<name>.cmd` shims.
  if (manager === "bun") {
    return run("bun", ["run", name, ...args], {
      cwd: project,
      capture: true,
      status: options.status,
    });
  }
  const suffix = process.platform === "win32" ? ".cmd" : "";
  return run(join(project, "node_modules", ".bin", `${name}${suffix}`), args, {
    cwd: project,
    capture: true,
    env: options.env,
    status: options.status,
  });
}

async function binSnapshot(project) {
  const binDir = join(project, "node_modules", ".bin");
  const entries = (await readdir(binDir)).sort();
  const snapshot = [];
  for (const entry of entries) {
    const entryPath = join(binDir, entry);
    const stat = await lstat(entryPath);
    if (stat.isSymbolicLink()) {
      snapshot.push(
        `${entry}:link:${(await readlink(entryPath)).replaceAll("\\", "/")}`,
      );
    } else {
      const content = (await readFile(entryPath, "utf8"))
        .split(project)
        .join("<PROJECT>")
        .replaceAll("\\", "/");
      snapshot.push(`${entry}:file:${content}`);
    }
  }
  return snapshot;
}

async function verifySelfHeal(
  normalProject,
  mainTarball,
  platformDir,
  selected,
) {
  const project = join(sandbox, "npm-self-heal");
  await mkdir(project, { recursive: true });
  await writeJson(join(project, "package.json"), {
    name: "utoo-launcher-npm-self-heal-e2e",
    private: true,
  });
  await run(
    "npm",
    [
      "install",
      "--ignore-scripts",
      "--omit=optional",
      "--no-audit",
      "--no-fund",
      fileSpec(mainTarball),
    ],
    { cwd: project },
  );

  const packageSegments = selected.packageName.split("/");
  await Promise.all([
    rm(join(project, "node_modules", ...packageSegments), {
      recursive: true,
      force: true,
    }),
    rm(join(project, "node_modules", "utoo", "node_modules", ...packageSegments), {
      recursive: true,
      force: true,
    }),
  ]);

  const expectedBins = await binSnapshot(normalProject);
  const binsBefore = await binSnapshot(project);
  if (JSON.stringify(binsBefore) !== JSON.stringify(expectedBins)) {
    throw new Error(
      `self-heal bin shims differ before repair\nnormal=${JSON.stringify(expectedBins)}\nheal=${JSON.stringify(binsBefore)}`,
    );
  }

  const fakeDir = join(sandbox, "fake-npm-bin");
  const fakeScript = join(fakeDir, "fake-npm.js");
  const fakeLog = join(fakeDir, "args.json");
  await mkdir(fakeDir, { recursive: true });
  await writeFile(
    fakeScript,
    [
      '"use strict";',
      'const fs = require("fs");',
      'const path = require("path");',
      "const args = process.argv.slice(2);",
      'const prefixArg = args.find((arg) => arg.startsWith("--prefix="));',
      "if (!prefixArg) process.exit(31);",
      'fs.writeFileSync(process.env.FAKE_NPM_LOG, JSON.stringify(args));',
      'const target = path.join(prefixArg.slice(9), "node_modules", ...process.env.FAKE_PACKAGE_NAME.split("/"));',
      "fs.mkdirSync(path.dirname(target), { recursive: true });",
      "fs.cpSync(process.env.FAKE_PLATFORM_DIR, target, { recursive: true });",
      "",
    ].join("\n"),
  );
  if (process.platform === "win32") {
    await writeFile(
      join(fakeDir, "npm.cmd"),
      `@echo off\r\n"${process.execPath}" "${fakeScript}" %*\r\n`,
    );
  } else {
    await writeFile(
      join(fakeDir, "npm"),
      `#!${process.execPath}\nrequire(${JSON.stringify(fakeScript)});\n`,
    );
    await chmod(join(fakeDir, "npm"), 0o755);
  }

  const env = {
    ...process.env,
    FAKE_NPM_LOG: fakeLog,
    FAKE_PACKAGE_NAME: selected.packageName,
    FAKE_PLATFORM_DIR: platformDir,
    PATH: `${fakeDir}${delimiter}${process.env.PATH || ""}`,
    UTOO_REGISTRY: "https://registry.example.test/",
  };
  const output = await localCommand(
    "npm",
    project,
    "utoo",
    ["--version"],
    { env },
  );
  assertIncludes(
    output,
    process.platform === "win32" ? process.version : "ARGV[1]=--version",
    "npm self-heal native execution",
  );
  assertIncludes(output, "utoo: repaired", "npm self-heal success");

  const npmArgs = JSON.parse(await readFile(fakeLog, "utf8"));
  if (!npmArgs.includes("--registry=https://registry.example.test")) {
    throw new Error(`self-heal registry was not normalized: ${npmArgs.join(" ")}`);
  }
  const nestedBinary = join(
    project,
    "node_modules",
    "utoo",
    "node_modules",
    ...packageSegments,
    "bin",
    selected.executable,
  );
  if (!(await exists(nestedBinary))) {
    throw new Error(`self-heal binary missing: ${nestedBinary}`);
  }

  const binsAfter = await binSnapshot(project);
  if (JSON.stringify(binsAfter) !== JSON.stringify(expectedBins)) {
    throw new Error(
      `self-heal bin shims differ after repair\nnormal=${JSON.stringify(expectedBins)}\nheal=${JSON.stringify(binsAfter)}`,
    );
  }
  const second = await localCommand("npm", project, "utoo", ["--version"], {
    env: { ...env, PATH: process.env.PATH || "" },
  });
  if (second.includes("utoo: repairing")) {
    throw new Error("second self-heal invocation called npm again");
  }
}

function assertIncludes(actual, expected, context) {
  if (!actual.includes(expected)) {
    throw new Error(
      `${context}: expected ${JSON.stringify(expected)} in:\n${actual}`,
    );
  }
}

async function verifyCommands(manager, project) {
  if (process.platform === "win32") {
    const printArgs =
      "console.log('ROOT=' + (process.env.UTOO_MANAGED_PACKAGE_ROOT || ''));" +
      "console.log(JSON.stringify(process.argv.slice(1)))";
    const output = await localCommand(manager, project, "utoo", [
      "-e",
      printArgs,
      "space arg",
      "你好",
    ]);
    if (!/ROOT=.+/.test(output)) {
      throw new Error(`${manager} managed root was empty:\n${output}`);
    }
    assertIncludes(output, '["space arg","你好"]', `${manager} argv`);
    await localCommand(manager, project, "utoo", ["-e", "process.exit(23)"], {
      status: 23,
    });
    assertIncludes(
      await localCommand(manager, project, "ut", ["--version"]),
      process.version,
      `${manager} ut alias`,
    );
    await writeFile(
      join(project, "x"),
      "console.log(JSON.stringify(process.argv.slice(2)))\n",
    );
    assertIncludes(
      await localCommand(manager, project, "utx", ["space arg", "你好"]),
      '["space arg","你好"]',
      `${manager} utx`,
    );
  } else {
    const output = await localCommand(manager, project, "utoo", [
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
    await localCommand(manager, project, "utoo", ["exit-23"], { status: 23 });
    assertIncludes(
      await localCommand(manager, project, "ut", ["alias"]),
      "ARGV[1]=alias",
      `${manager} ut alias`,
    );
    const utx = await localCommand(manager, project, "utx", [
      "space arg",
      "你好",
    ]);
    assertIncludes(utx, "ARGV[1]=x", `${manager} utx command`);
    assertIncludes(utx, "ARGV[3]=你好", `${manager} utx argv`);
  }
}

async function verifyManifest(manager, project) {
  if (manager === "yarn" && !(await exists(join(project, "node_modules")))) {
    const script =
      "const p=require('utoo/package.json');process.exit(p.scripts ? 1 : 0)";
    await run("yarn", ["node", "-e", script], { cwd: project });
    return;
  }
  const manifest = JSON.parse(
    await readFile(
      join(project, "node_modules", "utoo", "package.json"),
      "utf8",
    ),
  );
  if (manifest.scripts) throw new Error(`${manager} installed lifecycle scripts`);
}

async function verifyGlobal(manager, mainTarball) {
  if (manager !== "npm" && manager !== "pnpm") return;
  const prefix = join(sandbox, `${manager}-global`);
  await mkdir(prefix, { recursive: true });
  const spec = fileSpec(mainTarball);
  let binDir;

  if (manager === "npm") {
    await run("npm", [
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
    await mkdir(binDir, { recursive: true });
    await run(
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
  await mkdir(project, { recursive: true });
  const suffix = process.platform === "win32" ? ".cmd" : "";
  const output = await run(
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

try {
  const { mainTarball, platformDir, selected } = await buildPackages();
  for (const manager of managers) {
    const project = join(sandbox, `${manager}-local`);
    await mkdir(project, { recursive: true });
    process.stdout.write(
      `\n== ${manager} (${process.platform}-${process.arch}) ==\n`,
    );
    await installLocal(manager, mainTarball, project);
    await verifyCommands(manager, project);
    await verifyManifest(manager, project);
    if (manager === "npm") {
      await verifySelfHeal(project, mainTarball, platformDir, selected);
    }
    await verifyGlobal(manager, mainTarball);
    process.stdout.write(`PASS ${manager}\n`);
  }
} finally {
  try {
    await rm(sandbox, { recursive: true, force: true });
  } catch {
    // A Windows scanner can briefly retain a copied executable after exit.
  }
}
