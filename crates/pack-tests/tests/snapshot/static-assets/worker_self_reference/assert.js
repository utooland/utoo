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
const context = vm.createContext({ TURBOPACK: [] });
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
