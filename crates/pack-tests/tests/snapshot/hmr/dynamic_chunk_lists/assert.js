const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const registrations = fs
  .readdirSync(outputDir)
  .filter((file) => file.endsWith(".js"))
  .map((file) => fs.readFileSync(path.join(outputDir, file), "utf8"))
  .filter((content) => content.includes("_CHUNK_LISTS"));

const entryRegistrations = registrations.filter((content) =>
  /source:\s*["']entry["']/.test(content),
);
const dynamicRegistrations = registrations.filter((content) =>
  /source:\s*["']dynamic["']/.test(content),
);

assert.equal(
  entryRegistrations.length,
  1,
  "the runtime entry should have one HMR chunk list",
);
assert.equal(
  dynamicRegistrations.length,
  1,
  "a loaded dynamic chunk group should have its own HMR chunk list",
);

function readChunks(registration) {
  const match = registration.match(/chunks:\s*(\[[^\n]*\])/);
  assert.ok(match, "the HMR registration should contain a chunks array");
  return JSON.parse(match[1]);
}

const entryChunks = new Set(readChunks(entryRegistrations[0]));
const dynamicChunks = readChunks(dynamicRegistrations[0]);

assert.ok(dynamicChunks.length > 0, "the dynamic HMR list should not be empty");
assert.ok(
  dynamicChunks.every((chunk) => !entryChunks.has(chunk)),
  "dynamic payload chunks should not be duplicated in the entry HMR list",
);
