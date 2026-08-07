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
for (const asset of stats.assets) {
  if (!asset.name.endsWith(".js") || asset.name === "main.js") {
    continue;
  }

  globalThis.utooChunk_async_chunk_test = [];
  require(path.join(outputDir, asset.name));

  const factories = new Map();
  for (const registration of globalThis.utooChunk_async_chunk_test) {
    for (let index = 1; index < registration.length; index += 2) {
      const id = registration[index];
      const factory = registration[index + 1];
      if (typeof factory === "function") {
        factories.set(id, factory);
      }
    }
  }
  factoriesByChunk.set(asset.name, factories);
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
      Buffer.byteLength(factory.toString()) + 2,
      `stats module ${module.id} must use generated module factory size`,
    );
  }
}
