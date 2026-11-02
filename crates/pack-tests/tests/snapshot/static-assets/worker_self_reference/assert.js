const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const outputDir = path.join(__dirname, "output");
const files = fs
  .readdirSync(outputDir, { recursive: true })
  .filter((file) => file.endsWith(".js"));
assert(files.length >= 4, "expected entry, worker chunks, and worker bootstrap");

// Collect registrations without executing module factories or the worker bootstrap.
const context = vm.createContext({
  TURBOPACK: [],
  URL,
  location: { origin: "https://example.test" },
});
for (const file of files) {
  assert.match(file, /\.[0-9a-f]{8}\.js$/);
  const source = fs.readFileSync(path.join(outputDir, file), "utf8");
  if (source.startsWith('(globalThis["TURBOPACK"]')) {
    vm.runInContext(source, context, { filename: file });
  }
}

const registrations = context.TURBOPACK;
const bootstraps = registrations
  .filter((registration) => registration.length === 2)
  .map((registration) => registration[1]);
const entry = bootstraps.find((params) => params.workerChunks);
assert(entry, "entry bootstrap must register its worker chunks");
assert.equal(entry.workerChunks.length, 1);

const [workerId, workerChunks] = entry.workerChunks[0];
assert.match(workerId, /\/input\/worker\.js .*worker loader/);
assert(workerChunks.length >= 2, "worker needs factories and an evaluate chunk");
for (const chunk of [...entry.otherChunks, ...workerChunks]) {
  const chunkPath = typeof chunk === "string" ? chunk : chunk.path;
  assert(files.includes(chunkPath), `missing referenced chunk ${chunkPath}`);
}
assert(
  registrations.some((registration) => registration.includes(workerId)),
  "worker loader must be emitted as a module factory",
);
assert(
  bootstraps.some(
    (params) =>
      params.runtimeModuleIds.some((id) => id.includes("/input/worker.js ")) &&
      !params.workerChunks,
  ),
  "worker bootstrap must avoid embedding its own manifest",
);

// Run only the generated helper and loader, using a constructor that records URLs.
const factories = new Map();
for (const registration of registrations) {
  for (let i = 1; i + 1 < registration.length; i += 2) {
    factories.set(registration[i], registration[i + 1]);
  }
}
const helperId = [...factories.keys()].find((id) =>
  id.includes("/worker/browser/createWorker.ts "),
);
assert(helperId, "worker helper factory must be emitted");
const helperExports = {};
let createWorker;
const runtime = {
  s(bindings) {
    for (let i = 0; i < bindings.length; i += 3) {
      helperExports[bindings[i]] = bindings[i + 2];
    }
  },
  v(value) {
    createWorker = value;
  },
  r(id) {
    assert.equal(id, helperId);
    return helperExports;
  },
  W: new Map(entry.workerChunks),
  h: (chunkPath, basePath) => `${basePath}${chunkPath}`,
  b: "/assets/",
  X: "",
};
factories.get(helperId)(runtime);
factories.get(workerId)(runtime);
assert.equal(typeof createWorker, "function");

const urls = [];
class SharedWorker {
  constructor(url) {
    urls.push(url.href);
  }
}
createWorker(SharedWorker);
const originalUrl = urls.at(-1);
const params = JSON.parse(new URL(originalUrl).searchParams.get("params"));
assert.deepEqual(params[3], JSON.parse(JSON.stringify(entry.workerChunks)));

runtime.W.set("unrelated-worker", ["unrelated.12345678.js"]);
createWorker(SharedWorker);
assert.equal(
  urls.at(-1),
  originalUrl,
  "unrelated workers must preserve URL identity",
);
runtime.W.set("unrelated-worker", ["unrelated.87654321.js"]);
createWorker(SharedWorker);
assert.equal(
  urls.at(-1),
  originalUrl,
  "unrelated worker edits must preserve URL identity",
);

runtime.W.set(workerId, ["self.12345678.js"]);
createWorker(SharedWorker);
assert.notEqual(
  urls.at(-1),
  originalUrl,
  "reachable worker edits must change the URL",
);
