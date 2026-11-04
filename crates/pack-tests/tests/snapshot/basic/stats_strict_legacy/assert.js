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

assert.equal(stats.modules.length, 2, "exactly two ESM modules exercise the threshold");
assert.equal(chunkNames.size, 1, "both factories must share one chunk");

for (const chunkName of chunkNames) {
  const registrations = [];
  const sandbox = { utooChunk_stats_strict_legacy: registrations };
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
      if (grouped) groupedFactories++;
      assert.match(
        factory.toString(),
        /^function\b[^{]*\{\s*"use strict";/,
        "Chrome 41 Flat factories must retain their strict directives",
      );
    }
  }
  for (const registration of registrations) {
    collect(registration, 1);
  }
  factoriesByChunk.set(chunkName, factories);
}

assert.equal(groupedFactories, 0, "two legacy ESM factories must stay Flat");

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
