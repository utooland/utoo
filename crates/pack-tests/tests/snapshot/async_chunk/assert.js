const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const stats = JSON.parse(
  fs.readFileSync(path.join(outputDir, "stats.json"), "utf8"),
);

const moduleIds = stats.modules.map((module) => module.id);
assert.strictEqual(
  new Set(moduleIds).size,
  moduleIds.length,
  "webpack stats module ids must be unique",
);

const importModules = stats.modules.filter(
  (module) => module.name === "async_chunk/input/import.js",
);
assert.strictEqual(
  importModules.length,
  2,
  "stats module names must remain human-readable source paths",
);
assert.notStrictEqual(
  importModules[0].id,
  importModules[1].id,
  "different generated modules for one source must keep distinct runtime ids",
);

const factoriesByChunk = new Map();
function collectFactories(items, factories) {
  const ids = [];
  for (const item of items) {
    if (Array.isArray(item)) {
      assert.strictEqual(ids.length, 0, "expected strict groups before module ids");
      collectFactories(item, factories);
    } else if (typeof item === "function") {
      assert.ok(ids.length, "expected module ids before each factory");
      for (const id of ids) factories.set(id, item);
      ids.length = 0;
    } else {
      assert.ok(typeof item === "string" || typeof item === "number");
      ids.push(item);
    }
  }
  assert.strictEqual(ids.length, 0, "expected a factory after module ids");
}

for (const chunk of new Set(stats.modules.flatMap((module) => module.chunks))) {
  globalThis.utooChunk_async_chunk_test = [];
  require(path.join(outputDir, chunk));

  const factories = new Map();
  for (const registration of globalThis.utooChunk_async_chunk_test) {
    if (
      registration.length === 2 &&
      typeof registration[1] === "object" &&
      !Array.isArray(registration[1])
    ) {
      continue;
    }
    collectFactories(registration.slice(1), factories);
  }
  factoriesByChunk.set(chunk, factories);
}

for (const module of stats.modules) {
  for (const chunk of module.chunks) {
    const factories = factoriesByChunk.get(chunk);
    assert.ok(
      factories,
      `stats module ${module.id} points to non-module chunk ${chunk}`,
    );

    const factory = factories.get(module.id);
    assert.ok(
      factory,
      `stats module ${module.id} is not emitted in chunk ${chunk}`,
    );

    assert.strictEqual(
      module.size,
      // Chunk item code surrounds the emitted factory with parentheses.
      Buffer.byteLength(factory.toString()) + 2,
      `stats module ${module.id} must use generated module factory size`,
    );
  }
}
