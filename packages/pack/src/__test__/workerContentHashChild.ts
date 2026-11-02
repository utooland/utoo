import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import type { ConfigComplete } from "../config/types";

const [, , projectPath, scenario] = process.argv;
if (
  !projectPath ||
  !["self", "mutual", "ordinary", "self-sourcemap"].includes(scenario)
) {
  throw new Error("Usage: workerContentHashChild <projectPath> <scenario>");
}
const sourceDir = path.join(projectPath, "src");
const outputDir = path.join(projectPath, "dist");
const marker = "worker-content-hash-v1";
const changedMarker = "worker-content-hash-v2";
const dependency = "worker-dependency-v1";
const changedDependency = "worker-dependency-v2";
const sourceMaps = scenario === "self-sourcemap";

type Output = {
  hashes: Record<string, string>;
  source: Record<string, string>;
  entry: string;
};

function readOutput(): Output {
  const hashes: Record<string, string> = {};
  const source: Record<string, string> = {};
  function visit(directory: string) {
    for (const item of fs.readdirSync(directory, { withFileTypes: true })) {
      const filename = path.join(directory, item.name);
      if (item.isDirectory()) visit(filename);
      else {
        const name = path.relative(outputDir, filename).replaceAll("\\", "/");
        const content = fs.readFileSync(filename);
        hashes[name] = createHash("sha256").update(content).digest("hex");
        if (name.endsWith(".js")) source[name] = content.toString();
      }
    }
  }
  visit(outputDir);
  const entry = Object.keys(source).filter((name) =>
    /^main\.[0-9a-f]{8}\.js$/.test(name),
  );
  assert.equal(entry.length, 1, "Expected a content-hashed entry bootstrap");
  return { hashes, source, entry: entry[0] };
}

function containing(output: Output, value: string): string[] {
  const files = Object.keys(output.source).filter((name) =>
    output.source[name].includes(value),
  );
  assert.ok(files.length > 0, `No output contains ${value}`);
  for (const name of files) {
    assert.match(name, /\.[0-9a-f]{8}\.js$/, `Unhashed worker output: ${name}`);
  }
  return files.sort();
}

function assertChanged(
  previous: Output,
  next: Output,
  oldValue: string,
  newValue: string,
) {
  assert.notEqual(
    previous.entry,
    next.entry,
    "Worker changes must invalidate the entry's embedded worker URLs",
  );
  assert.notDeepEqual(
    containing(previous, oldValue),
    containing(next, newValue),
    "Changed worker content must receive new chunk names",
  );
  const outputText = Object.values(next.source).join("\n");
  assert.ok(
    !outputText.includes(oldValue),
    "Persistent cache returned old code",
  );
  for (const name of Object.keys(previous.source)) {
    if (!(name in next.source)) {
      assert.ok(
        !outputText.includes(name),
        `Output retained a stale chunk reference: ${name}`,
      );
    }
  }
}

async function main() {
  // Only pack's addon uses this override. Other napi-rs packages must resolve
  // their own native libraries instead of trying to load the pack addon.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { build } = await import("../commands/build");
  fs.mkdirSync(sourceDir, { recursive: true });
  const workerTarget =
    scenario === "self" || sourceMaps ? "worker.js" : "other-worker.js";
  const workerSource = `import { value } from "./dependency.js";
export function spawnWorker() {
  return new Worker(new URL("./${workerTarget}", import.meta.url), { type: "module" });
}
globalThis.spawnWorker = spawnWorker;
self.onmessage = () => postMessage("${marker}:" + value);
`;
  const sources: Record<string, string> = {
    "index.js": `globalThis.worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });\n`,
    "dependency.js": `export const value = "${dependency}";\n`,
    "worker.js":
      scenario === "ordinary"
        ? `import { value } from "./dependency.js";\nself.onmessage = () => postMessage("${marker}:" + value);\n`
        : workerSource,
  };
  if (scenario === "mutual") {
    sources["other-worker.js"] = `export function spawnWorker() {
  return new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
}
globalThis.spawnWorker = spawnWorker;
self.onmessage = () => postMessage("other-worker");
`;
  }
  for (const [name, content] of Object.entries(sources)) {
    fs.writeFileSync(path.join(sourceDir, name), content);
  }

  const config: ConfigComplete = {
    mode: "production",
    entry: [{ name: "main", import: "./src/index.js" }],
    output: {
      path: outputDir,
      filename: "[name].[contenthash:8].js",
      chunkFilename: "chunks/[name].[contenthash:8].js",
      clean: true,
    },
    sourceMaps,
    cacheDirectory: path.join(projectPath, ".cache"),
    optimization: { minify: sourceMaps },
  };
  async function compile(persistentCaching: boolean) {
    await build(
      {
        config: { ...config, persistentCaching },
        buildId: "worker-content-hash-test",
        tracing: false,
      },
      projectPath,
      projectPath,
    );
    return readOutput();
  }

  const cold = await compile(false);
  containing(cold, marker);
  containing(cold, dependency);
  const cached = await compile(true);
  const cacheFiles = fs.readdirSync(config.cacheDirectory!, {
    recursive: true,
  });
  assert.ok(
    cacheFiles.some((name) => path.basename(name) === "CURRENT") &&
      cacheFiles.some((name) => name.endsWith(".sst")),
    "Expected a persisted Turbo Tasks snapshot",
  );
  assert.deepEqual(
    cached.hashes,
    cold.hashes,
    "Caching changed emitted content",
  );
  const warm = await compile(true);
  assert.deepEqual(
    warm.hashes,
    cold.hashes,
    "Unchanged content must be stable",
  );

  fs.writeFileSync(
    path.join(sourceDir, "worker.js"),
    sources["worker.js"].replace(marker, changedMarker),
  );
  const changedWorker = await compile(true);
  assertChanged(warm, changedWorker, marker, changedMarker);

  fs.writeFileSync(
    path.join(sourceDir, "dependency.js"),
    sources["dependency.js"].replace(dependency, changedDependency),
  );
  const changedImport = await compile(true);
  assertChanged(changedWorker, changedImport, dependency, changedDependency);
  const unchanged = await compile(true);
  assert.deepEqual(
    unchanged.hashes,
    changedImport.hashes,
    "A warm rebuild changed content hashes without a source edit",
  );

  fs.writeFileSync(path.join(sourceDir, "worker.js"), sources["worker.js"]);
  fs.writeFileSync(
    path.join(sourceDir, "dependency.js"),
    sources["dependency.js"],
  );
  const restored = await compile(true);
  assert.deepEqual(
    restored.hashes,
    cold.hashes,
    "Restoring source content must restore its original hashes",
  );

  if (sourceMaps) {
    fs.writeFileSync(
      path.join(sourceDir, "dependency.js"),
      `// Map-only source comment.\n${sources["dependency.js"]}`,
    );
    const mapOnly = await compile(true);
    const [oldWorker] = containing(restored, marker);
    const [newWorker] = containing(mapOnly, marker);
    const withoutMapUrl = (source: string) =>
      source.replace(/\n?\/\/# sourceMappingURL=[^\n]*(?:\n|$)/g, "");
    assert.equal(
      withoutMapUrl(mapOnly.source[newWorker]),
      withoutMapUrl(restored.source[oldWorker]),
      "A source-map-only edit changed the minified worker code",
    );
    assert.notEqual(
      oldWorker,
      newWorker,
      "Map changes must invalidate worker URLs",
    );
    assert.notEqual(
      restored.entry,
      mapOnly.entry,
      "Map changes must invalidate the entry's worker manifest",
    );
    assert.ok(
      restored.hashes[`${oldWorker}.map`],
      "Missing original paired map",
    );
    assert.ok(mapOnly.hashes[`${newWorker}.map`], "Missing updated paired map");
    const unchangedMap = await compile(true);
    assert.deepEqual(
      unchangedMap.hashes,
      mapOnly.hashes,
      "An unchanged source map must retain stable content hashes",
    );
  }
  console.log(`__WORKER_CONTENT_HASH__${JSON.stringify({ scenario })}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
