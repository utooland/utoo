const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const outputDir = path.join(__dirname, "output");
const stats = JSON.parse(
  fs.readFileSync(path.join(outputDir, "stats.json"), "utf8"),
);
const chunkNames = new Set(stats.modules.flatMap((module) => module.chunks));
const factoriesByChunk = new Map();
let groupedFactories = 0;
let flatFactories = 0;

assert.equal(stats.modules.length, 4, "three ESM modules and one sloppy CJS");
assert.equal(chunkNames.size, 1, "all four factories must share one chunk");

for (const chunkName of chunkNames) {
  const registrations = [];
  const sandbox = { utooChunk_stats_strict_mixed: registrations };
  sandbox.self = sandbox;
  vm.runInNewContext(
    fs.readFileSync(path.join(outputDir, chunkName), "utf8"),
    sandbox,
    { filename: chunkName },
  );

  const factories = new Map();
  function collect(entries, offset = 0, grouped = false) {
    for (let index = offset; index < entries.length; ) {
      const id = entries[index++];
      if (Array.isArray(id)) {
        collect(id, 0, true);
        continue;
      }
      const factory = entries[index++];
      assert.equal(typeof factory, "function", `missing factory ${id}`);
      factories.set(id, factory);
      if (grouped) {
        groupedFactories++;
        assert.doesNotMatch(factory.toString(), /\{\s*"use strict";/);
      } else {
        flatFactories++;
      }
    }
  }
  for (const registration of registrations) {
    collect(registration, 1);
  }
  factoriesByChunk.set(chunkName, factories);
}

assert.equal(groupedFactories, 3, "Mixed mode must group the three ESM factories");
assert.equal(flatFactories, 1, "the sloppy CJS factory must remain outside");

for (const module of stats.modules) {
  for (const chunkName of module.chunks) {
    const factory = factoriesByChunk.get(chunkName)?.get(module.id);
    assert.equal(typeof factory, "function", `missing module ${module.id}`);
    assert.equal(
      module.size,
      Buffer.byteLength(factory.toString()) + 2,
      `stats module ${module.id} must match its emitted parenthesized factory`,
    );
  }
}
