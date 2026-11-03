const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const stats = JSON.parse(
  fs.readFileSync(path.join(outputDir, "stats.json"), "utf8"),
);
assert.ok(stats.modules.length, "expected Node module stats");

function collectFactories(items, factories) {
  const ids = [];
  for (const item of items) {
    if (Array.isArray(item)) {
      assert.equal(ids.length, 0, "expected strict groups before module ids");
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
  assert.equal(ids.length, 0, "expected a factory after module ids");
}

const factoriesByChunk = new Map();
for (const chunk of new Set(stats.modules.flatMap((module) => module.chunks))) {
  const registration = require(path.join(outputDir, chunk));
  assert.ok(Array.isArray(registration), "expected Node module.exports factories");
  const factories = new Map();
  collectFactories(registration, factories);
  factoriesByChunk.set(chunk, factories);
}

for (const module of stats.modules) {
  for (const chunk of module.chunks) {
    const factory = factoriesByChunk.get(chunk).get(module.id);
    assert.equal(typeof factory, "function", `missing factory for ${module.id}`);
    assert.equal(
      module.size,
      // Chunk item code surrounds the emitted factory with parentheses.
      Buffer.byteLength(factory.toString()) + 2,
      `stats module ${module.id} must use generated module factory size`,
    );
  }
}
