import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const runtimeDirectory = path.dirname(scriptDirectory);
const nodePolyfillsDirectory = path.join(
  runtimeDirectory,
  "src",
  "node-polyfills",
);
const generatedDirectory = path.join(nodePolyfillsDirectory, "generated");
const stagingDirectory = `${generatedDirectory}.tmp`;

const runtimePackage = require("../package.json");
const stdlibPackage = require("node-stdlib-browser/package.json");
const stdlibPackagePath = require.resolve("node-stdlib-browser/package.json");
const stdlibAliases = require("node-stdlib-browser");

const expectedVersion = runtimePackage.devDependencies["node-stdlib-browser"];

if (stdlibPackage.version !== expectedVersion) {
  throw new Error(
    `Expected node-stdlib-browser@${expectedVersion}, but resolved ${stdlibPackage.version}`,
  );
}

const EXPECTED_ALIAS_COUNT = 99;
const LEGACY_EMPTY_POLYFILL = "<legacy-empty>";
const EMPTY_POLYFILL_PATH = "@utoo/pack-runtime/node-polyfills/empty/empty.js";
const EMBEDDED_NODE_MODULES_PATH =
  "@utoo/pack-runtime/node-polyfills/generated/node_modules";
const sourceNodeModulesDirectory = realpathSync(
  path.dirname(path.dirname(stdlibPackagePath)),
);

const legacyEmptyModules = [
  "async_hooks",
  "child_process",
  "cluster",
  "dgram",
  "diagnostics_channel",
  "dns",
  "fs",
  "fs/promises",
  "http2",
  "inspector",
  "module",
  "net",
  "perf_hooks",
  "readline",
  "repl",
  "tls",
  "trace_events",
  "v8",
  "wasi",
  "worker_threads",
];

const aliases = new Map(Object.entries(stdlibAliases));

for (const moduleName of legacyEmptyModules) {
  aliases.set(moduleName, LEGACY_EMPTY_POLYFILL);
  aliases.set(`node:${moduleName}`, LEGACY_EMPTY_POLYFILL);
}

const timersRequire = createRequire(
  require.resolve("timers-browserify/package.json"),
);
aliases.set("setimmediate", timersRequire.resolve("setimmediate"));

if (aliases.size !== EXPECTED_ALIAS_COUNT) {
  throw new Error(
    `Expected ${EXPECTED_ALIAS_COUNT} aliases, but resolved ${aliases.size}`,
  );
}

function compareStrings(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function toPortablePath(value) {
  return value.split(path.sep).join("/");
}

function relativePathInside(root, target, description) {
  const relative = path.relative(root, target);

  if (
    relative === "" ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    throw new Error(`${description} is outside ${root}: ${target}`);
  }

  return relative;
}

function readPackage(packageDirectory) {
  const packagePath = path.join(packageDirectory, "package.json");

  if (!existsSync(packagePath)) {
    return undefined;
  }

  const manifest = JSON.parse(readFileSync(packagePath, "utf8"));

  if (
    typeof manifest.name !== "string" ||
    typeof manifest.version !== "string"
  ) {
    return undefined;
  }

  return {
    directory: packageDirectory,
    manifest,
    path: packagePath,
  };
}

function findPackage(target) {
  let current;

  try {
    current = statSync(target).isDirectory() ? target : path.dirname(target);
  } catch {
    // Some aliases intentionally omit the .js extension
    current = path.dirname(target);
  }

  for (;;) {
    const packageInfo = readPackage(current);

    if (packageInfo) {
      return packageInfo;
    }

    const parent = path.dirname(current);

    if (parent === current) {
      throw new Error(`Unable to find the package containing ${target}`);
    }

    current = parent;
  }
}

function resolveDependency(packageInfo, dependencyName, optional) {
  const packageRequire = createRequire(packageInfo.path);

  try {
    return findPackage(
      packageRequire.resolve(`${dependencyName}/package.json`),
    );
  } catch {
    try {
      const resolvedEntry = packageRequire.resolve(dependencyName);

      if (!path.isAbsolute(resolvedEntry)) {
        throw new Error(
          `${dependencyName} resolved to the Node.js builtin ${resolvedEntry}`,
        );
      }

      return findPackage(resolvedEntry);
    } catch (error) {
      if (optional) {
        return undefined;
      }

      throw new Error(
        `Unable to resolve ${dependencyName} from ${packageInfo.manifest.name}@${packageInfo.manifest.version}`,
        { cause: error },
      );
    }
  }
}

function collectPackages(aliasTargets) {
  const queue = aliasTargets.map((target) => findPackage(target));
  const packages = new Map();

  while (queue.length > 0) {
    const packageInfo = queue.shift();
    const realDirectory = realpathSync(packageInfo.directory);

    if (packages.has(realDirectory)) {
      continue;
    }

    const relativeDirectory = relativePathInside(
      sourceNodeModulesDirectory,
      realDirectory,
      `${packageInfo.manifest.name}'s directory`,
    );

    packages.set(realDirectory, {
      ...packageInfo,
      directory: realDirectory,
      relativeDirectory,
    });

    const optionalDependencies = new Set([
      ...Object.keys(packageInfo.manifest.optionalDependencies ?? {}),
      ...Object.entries(packageInfo.manifest.peerDependenciesMeta ?? {})
        .filter(([, metadata]) => metadata.optional === true)
        .map(([dependencyName]) => dependencyName),
    ]);

    const dependencies = new Set([
      ...Object.keys(packageInfo.manifest.dependencies ?? {}),
      ...Object.keys(packageInfo.manifest.optionalDependencies ?? {}),
      ...Object.keys(packageInfo.manifest.peerDependencies ?? {}),
    ]);

    for (const dependencyName of [...dependencies].sort(compareStrings)) {
      const dependency = resolveDependency(
        packageInfo,
        dependencyName,
        optionalDependencies.has(dependencyName),
      );

      if (dependency) {
        queue.push(dependency);
      }
    }
  }

  return [...packages.values()].sort((left, right) =>
    compareStrings(left.relativeDirectory, right.relativeDirectory),
  );
}

function toEmbeddedAlias(moduleName, target) {
  if (target === LEGACY_EMPTY_POLYFILL) {
    return EMPTY_POLYFILL_PATH;
  }

  if (typeof target !== "string") {
    throw new TypeError(`Alias ${moduleName} has an invalid target`);
  }

  const relativeTarget = relativePathInside(
    sourceNodeModulesDirectory,
    target,
    `Alias ${moduleName}`,
  );

  return `${EMBEDDED_NODE_MODULES_PATH}/${toPortablePath(relativeTarget)}`;
}

function validateGeneratedAliases(generatedAliases) {
  const embeddedPrefix = `${EMBEDDED_NODE_MODULES_PATH}/`;

  for (const [moduleName, target] of Object.entries(generatedAliases)) {
    if (target === EMPTY_POLYFILL_PATH) {
      continue;
    }

    if (!target.startsWith(embeddedPrefix)) {
      throw new Error(`Alias ${moduleName} has an invalid target: ${target}`);
    }

    const relativeTarget = target.slice(embeddedPrefix.length);
    const diskTarget = path.join(
      stagingDirectory,
      "node_modules",
      ...relativeTarget.split("/"),
    );

    try {
      require.resolve(diskTarget);
    } catch (error) {
      throw new Error(
        `Generated alias ${moduleName} cannot resolve ${diskTarget}`,
        { cause: error },
      );
    }
  }
}

const excludedDirectories = new Set([
  ".github",
  "benchmark",
  "benchmarks",
  "coverage",
  "doc",
  "docs",
  "example",
  "examples",
  "test",
  "tests",
]);

function shouldCopyPackagePath(packageDirectory, sourcePath) {
  const relative = path.relative(packageDirectory, sourcePath);

  if (relative === "") {
    return true;
  }

  const segments = relative.split(path.sep);

  // Nested package instances are collected and copied separately, preserving
  // their original npm layout
  if (
    segments.includes("node_modules") ||
    segments.some((segment) => excludedDirectories.has(segment.toLowerCase()))
  ) {
    return false;
  }

  const stats = lstatSync(sourcePath);

  if (stats.isSymbolicLink()) {
    throw new Error(`Refusing to copy symbolic link ${sourcePath}`);
  }

  if (stats.isDirectory()) {
    return true;
  }

  if (!stats.isFile()) {
    return false;
  }

  const fileName = path.basename(sourcePath).toLowerCase();

  return (
    /\.(?:[cm]?js(?:\.map)?|json|wasm)$/.test(fileName) ||
    fileName.includes("license") ||
    fileName.includes("licence") ||
    fileName.startsWith("copying") ||
    fileName.startsWith("notice")
  );
}

const sortedAliases = [...aliases.entries()].sort(([left], [right]) =>
  compareStrings(left, right),
);

const generatedAliases = Object.fromEntries(
  sortedAliases.map(([moduleName, target]) => [
    moduleName,
    toEmbeddedAlias(moduleName, target),
  ]),
);

const runtimePackages = collectPackages(
  [...new Set(aliases.values())].filter(
    (target) => target !== LEGACY_EMPTY_POLYFILL,
  ),
);

const generatedPackages = Object.fromEntries(
  runtimePackages.map((packageInfo) => [
    `node_modules/${toPortablePath(packageInfo.relativeDirectory)}`,
    `${packageInfo.manifest.name}@${packageInfo.manifest.version}`,
  ]),
);

rmSync(stagingDirectory, { force: true, recursive: true });

mkdirSync(path.join(stagingDirectory, "node_modules"), { recursive: true });

try {
  for (const packageInfo of runtimePackages) {
    const destination = path.join(
      stagingDirectory,
      "node_modules",
      packageInfo.relativeDirectory,
    );
    cpSync(packageInfo.directory, destination, {
      filter: (sourcePath) =>
        shouldCopyPackagePath(packageInfo.directory, sourcePath),
      recursive: true,
    });
  }

  validateGeneratedAliases(generatedAliases);

  writeFileSync(
    path.join(stagingDirectory, "manifest.json"),
    `${JSON.stringify({ source: `node-stdlib-browser@${stdlibPackage.version}`, aliases: generatedAliases, packages: generatedPackages }, null, 2)}\n`,
  );

  rmSync(generatedDirectory, {
    force: true,
    recursive: true,
  });

  renameSync(stagingDirectory, generatedDirectory);
} finally {
  rmSync(stagingDirectory, {
    force: true,
    recursive: true,
  });
}

const samples = [
  "buffer",
  "node:buffer",
  "fs",
  "fs/promises",
  "node:fs/promises",
  "timers/promises",
  "setimmediate",
];

console.log(
  JSON.stringify(
    {
      source: `node-stdlib-browser@${stdlibPackage.version}`,
      aliasCount: aliases.size,
      packageCount: runtimePackages.length,
      output: toPortablePath(
        path.relative(runtimeDirectory, generatedDirectory),
      ),
      samples: Object.fromEntries(
        samples.map((moduleName) => [moduleName, generatedAliases[moduleName]]),
      ),
    },
    null,
    2,
  ),
);
