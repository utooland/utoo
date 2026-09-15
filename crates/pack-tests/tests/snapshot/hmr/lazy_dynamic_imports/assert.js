const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const files = fs.readdirSync(outputDir).filter((file) => file.endsWith(".js"));

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

// Until the manifest chunk is requested the target module is not compiled, so
// none of the emitted chunks contain it.
for (const file of files) {
  const content = fs.readFileSync(path.join(outputDir, file), "utf8");
  assert.ok(
    !content.includes("lazy-target-marker"),
    `${file} must not contain the lazily compiled module`,
  );
}
