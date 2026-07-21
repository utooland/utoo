const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const javascriptAssets = fs
  .readdirSync(outputDir)
  .filter((file) => file.endsWith(".js"))
  .map((file) => ({
    file,
    content: fs.readFileSync(path.join(outputDir, file), "utf8"),
  }));
const registrations = javascriptAssets.filter(({ content }) =>
  content.includes("_CHUNK_LISTS"),
);

const entryRegistrations = registrations.filter(({ content }) =>
  /source:\s*["']entry["']/.test(content),
);
const dynamicRegistrations = registrations.filter(({ content }) =>
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

function readChunkVersions(registration) {
  const match = registration.match(/chunkVersions:\s*(\{[^\n]*\})/);
  assert.ok(match, "the HMR registration should contain chunk versions");
  return JSON.parse(match[1]);
}

const entryChunks = new Set(readChunks(entryRegistrations[0].content));
const dynamicChunks = readChunks(dynamicRegistrations[0].content);
const dynamicChunkVersions = readChunkVersions(dynamicRegistrations[0].content);

assert.ok(dynamicChunks.length > 0, "the dynamic HMR list should not be empty");
assert.ok(
  dynamicChunks.every((chunk) => !entryChunks.has(chunk)),
  "dynamic payload chunks should not be duplicated in the entry HMR list",
);
assert.deepEqual(
  Object.keys(dynamicChunkVersions).sort(),
  [...dynamicChunks].sort(),
  "every dynamic member must carry the content version used for its URL",
);
assert.ok(
  Object.values(dynamicChunkVersions).every(
    (version) => typeof version === "string" && version.length > 0,
  ),
  "dynamic member content versions must be non-empty strings",
);

const asyncLoader = javascriptAssets.find(({ content }) =>
  content.includes("return parentImport("),
);
assert.ok(asyncLoader, "the fixture should emit an async loader");
const loaderMatch = asyncLoader.content.match(
  /return Promise\.all\((\[[\s\S]*?\])\.map/,
);
assert.ok(loaderMatch, "the async loader should contain a chunk array");
const loaderChunks = JSON.parse(loaderMatch[1]);
assert.deepEqual(
  loaderChunks,
  [dynamicRegistrations[0].file],
  "the async loader must fetch only the HMR-list bootstrap before content",
);
