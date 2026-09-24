const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const files = fs.readdirSync(outputDir).filter((file) => file.endsWith(".js"));

// A dynamic chunk group already registers its own HMR list. The lazy manifest
// must not wrap that list in another one, since nested list updates panic.
const chunkListFiles = new Set(
  files.filter((file) =>
    fs
      .readFileSync(path.join(outputDir, file), "utf8")
      .startsWith('(globalThis["TURBOPACK_CHUNK_LISTS"]'),
  ),
);
const dynamicChunkLists = [...chunkListFiles].filter((file) =>
  fs.readFileSync(path.join(outputDir, file), "utf8").includes('source: "dynamic"'),
);
assert.equal(
  dynamicChunkLists.length,
  1,
  "the lazy chunk group must retain its own dynamic HMR list",
);
for (const file of chunkListFiles) {
  const content = fs.readFileSync(path.join(outputDir, file), "utf8");
  for (const nestedList of chunkListFiles) {
    assert.ok(
      !content.includes(`"${nestedList}"`),
      `${file} must not track the HMR chunk list ${nestedList}`,
    );
  }
}

// The dynamic import is served through a manifest chunk whose file name carries
// the activation key, so a request for it can activate the lazy compilation.
const keyedFiles = files.filter((file) =>
  /lazy-compilation-[0-9a-f]{16}/.test(file),
);
const manifestChunks = keyedFiles.filter((file) =>
  fs
    .readFileSync(path.join(outputDir, file), "utf8")
    .includes("lazy compilation proxy"),
);
assert.equal(manifestChunks.length, 1);
assert.ok(
  fs
    .readFileSync(path.join(outputDir, manifestChunks[0]), "utf8")
    .includes(`"${dynamicChunkLists[0]}"`),
  "the lazy manifest must load the dynamic HMR list",
);

// Until the manifest chunk is requested the target module is not compiled, so
// none of the emitted chunks contain it.
for (const file of files) {
  const content = fs.readFileSync(path.join(outputDir, file), "utf8");
  assert.ok(
    !content.includes("lazy-target-marker"),
    `${file} must not contain the lazily compiled module`,
  );
}
