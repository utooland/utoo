import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import type { ConfigComplete } from "../config/types";

const [, , project, scenario] = process.argv;
assert(
  project &&
    ["self", "mutual", "ordinary", "self-sourcemap"].includes(scenario),
);
const input = path.join(project, "src");
const output = path.join(project, "dist");
const sourceMaps = scenario === "self-sourcemap";
const marker = "worker-content-v1";
const dependency = "worker-dependency-v1";
type Output = {
  files: Record<string, string>;
  entry: string;
  sharedUrl: string;
};

async function readOutput(): Promise<Output> {
  // Fixtures emit only text assets. Compare their complete bodies and filenames.
  const files = Object.fromEntries(
    fs.readdirSync(output, { recursive: true }).flatMap((name) => {
      const file = path.join(output, name);
      if (name.endsWith(".js")) assert.match(name, /\.[0-9a-f]{8}\.js$/);
      return fs.statSync(file).isFile()
        ? [[name.replaceAll("\\", "/"), fs.readFileSync(file, "utf8")]]
        : [];
    }),
  );
  const entry = Object.keys(files).find((name) =>
    /^main\.[0-9a-f]{8}\.js$/.test(name),
  );
  assert(entry, "Missing content-hashed entry");
  const errors: unknown[] = [];
  let currentScript: { src: string; getAttribute(): string } | undefined;
  const context: Record<string, unknown> = {
    URL,
    location: { origin: "https://example.test" },
    document: {
      get currentScript() {
        return currentScript;
      },
      querySelectorAll: () => [],
      createElement: () => ({ addEventListener() {} }),
      head: {
        appendChild(script: {
          src: string;
          onload?(): void;
          onerror?(error: unknown): void;
        }) {
          queueMicrotask(() => {
            try {
              execute(script.src);
              script.onload?.();
            } catch (error) {
              errors.push(error);
              script.onerror?.(error);
            }
          });
        },
      },
    },
    Worker: class Worker {},
    SharedWorker: class SharedWorker {
      url: string;
      constructor(url: URL) {
        this.url = url.href;
      }
    },
  };
  context.self = context;
  function execute(src: string) {
    const name = new URL(src, "https://example.test").pathname.slice(
      "/assets/".length,
    );
    const previous = currentScript;
    currentScript = { src, getAttribute: () => src };
    try {
      vm.runInNewContext(files[name], context, { timeout: 1000 });
    } finally {
      currentScript = previous;
    }
  }
  for (const match of files["index.html"].matchAll(
    /<script\b[^>]*src="([^"]+)"/g,
  )) {
    execute(match[1]);
  }
  // Default entry bootstraps load initial JS chunks asynchronously.
  await new Promise(setImmediate);
  assert.deepEqual(errors, []);
  const sharedUrl = (context.shared as { url: string }).url;
  const emitted = (url: string) => {
    const name = new URL(url, "https://example.test/assets/").pathname.slice(
      "/assets/".length,
    );
    assert(Object.hasOwn(files, name), "Missing worker output: " + name);
  };
  emitted(sharedUrl);
  const params = JSON.parse(new URL(sharedUrl).searchParams.get("params")!);
  for (const url of params[0]) emitted(url);
  for (const [, chunks] of params[3] ?? [])
    for (const chunk of chunks)
      emitted(typeof chunk === "string" ? chunk : chunk.path);
  return { files, entry, sharedUrl };
}

function containing(result: Output, value: string): string[] {
  const names = Object.keys(result.files)
    .filter(
      (name) => name.endsWith(".js") && result.files[name].includes(value),
    )
    .sort();
  assert(names.length, "Missing worker content: " + value);
  for (const name of names) assert.match(name, /\.[0-9a-f]{8}\.js$/);
  return names;
}

function changed(before: Output, after: Output, from: string, to: string) {
  assert.notEqual(
    before.entry,
    after.entry,
    "Worker change must invalidate entry",
  );
  assert.notEqual(
    before.sharedUrl,
    after.sharedUrl,
    "Reachable worker change must invalidate SharedWorker",
  );
  assert.notDeepEqual(containing(before, from), containing(after, to));
  const js = Object.entries(after.files)
    .filter(([name]) => name.endsWith(".js"))
    .map(([, body]) => body)
    .join("\n");
  assert(!js.includes(from), "Cache returned stale worker code");
  for (const name of Object.keys(before.files)) {
    if (name.endsWith(".js") && !(name in after.files))
      assert(!js.includes(name), "Stale chunk URL: " + name);
  }
}

async function main() {
  // The override belongs to pack only; domparser-rs must load its own addon.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { build } = await import("../commands/build");
  const target =
    scenario === "self" || sourceMaps ? "worker.js" : "other-worker.js";
  const sources: Record<string, string> = {
    "index.js": [
      'globalThis.worker = new Worker(new URL("./worker.js", import.meta.url));',
      'globalThis.shared = new SharedWorker(new URL("./worker.js", import.meta.url));',
      'globalThis.unrelatedWorker = () => new Worker(new URL("./unrelated.js", import.meta.url));',
    ].join("\n"),
    "dependency.js": 'export const value = "' + dependency + '";\n',
    "unrelated.js": 'self.onmessage = () => postMessage("unrelated-v1");\n',
    "worker.js": [
      'import { value } from "./dependency.js";',
      'self.onmessage = () => postMessage("' + marker + ':" + value);',
      ...(scenario === "ordinary"
        ? []
        : [
            'globalThis.spawnWorker = () => new Worker(new URL("./' +
              target +
              '", import.meta.url));',
          ]),
    ].join("\n"),
    ...(scenario === "mutual"
      ? {
          "other-worker.js":
            'globalThis.spawnWorker = () => new Worker(new URL("./worker.js", import.meta.url));\n',
        }
      : {}),
  };
  fs.mkdirSync(input, { recursive: true });
  const write = (name: string, body: string) =>
    fs.writeFileSync(path.join(input, name), body);
  for (const [name, body] of Object.entries(sources)) write(name, body);
  const config: ConfigComplete = {
    mode: "production",
    entry: [{ name: "main", import: "./src/index.js", html: {} }],
    output: {
      path: output,
      filename: "[name].[contenthash:8].js",
      chunkFilename: "chunks/[name].[contenthash:8].js",
      publicPath: "/assets/",
      clean: true,
    },
    sourceMaps,
    cacheDirectory: path.join(project, ".cache"),
    optimization: { minify: sourceMaps },
  };
  async function compile(persistentCaching = true) {
    await build(
      {
        config: { ...config, persistentCaching },
        buildId: "worker-content-hash-test",
        tracing: false,
      },
      project,
      project,
    );
    return readOutput();
  }

  const cold = await compile(false);
  containing(cold, marker);
  containing(cold, dependency);
  const cached = await compile();
  const cacheFiles = fs.readdirSync(config.cacheDirectory!, {
    recursive: true,
  });
  assert(
    cacheFiles.some((name) => path.basename(name) === "CURRENT") &&
      cacheFiles.some((name) => name.endsWith(".sst")),
    "Expected persisted Turbo Tasks snapshot",
  );
  assert.deepEqual(cached.files, cold.files);
  const warm = await compile();
  assert.deepEqual(warm, cold, "Unchanged content must stay stable");

  write("worker.js", sources["worker.js"].replace(marker, "worker-content-v2"));
  const workerChanged = await compile();
  changed(warm, workerChanged, marker, "worker-content-v2");
  write(
    "dependency.js",
    sources["dependency.js"].replace(dependency, "worker-dependency-v2"),
  );
  const importChanged = await compile();
  changed(workerChanged, importChanged, dependency, "worker-dependency-v2");

  write("unrelated.js", sources["unrelated.js"].replace("v1", "v2"));
  const unrelated = await compile();
  assert.notDeepEqual(
    containing(importChanged, "unrelated-v1"),
    containing(unrelated, "unrelated-v2"),
  );
  assert.equal(
    unrelated.sharedUrl,
    importChanged.sharedUrl,
    "Unrelated worker must preserve SharedWorker identity",
  );
  for (const name of containing(importChanged, "worker-content-v2")) {
    assert.equal(
      unrelated.files[name],
      importChanged.files[name],
      "Unrelated edit changed worker code or hashes",
    );
  }
  for (const name of ["worker.js", "dependency.js", "unrelated.js"])
    write(name, sources[name]);
  const restored = await compile();
  assert.deepEqual(
    restored,
    cold,
    "Restoring source must restore hashes and URLs",
  );

  if (sourceMaps) {
    write(
      "dependency.js",
      "// Map-only source comment.\n" + sources["dependency.js"],
    );
    const mapOnly = await compile();
    const [oldWorker] = containing(restored, marker);
    const [newWorker] = containing(mapOnly, marker);
    const withoutMap = (code: string) =>
      code.replace(/\n?\/\/# sourceMappingURL=[^\n]*(?:\n|$)/g, "");
    assert.equal(
      withoutMap(mapOnly.files[newWorker]),
      withoutMap(restored.files[oldWorker]),
    );
    assert.notEqual(
      oldWorker,
      newWorker,
      "Map edit must invalidate worker URL",
    );
    assert.notEqual(
      restored.entry,
      mapOnly.entry,
      "Map edit must invalidate entry",
    );
    assert.notEqual(
      restored.sharedUrl,
      mapOnly.sharedUrl,
      "Map edit must invalidate SharedWorker",
    );
    assert(
      restored.files[oldWorker + ".map"] && mapOnly.files[newWorker + ".map"],
    );
    assert.deepEqual(
      await compile(),
      mapOnly,
      "Unchanged source map must stay stable",
    );
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
