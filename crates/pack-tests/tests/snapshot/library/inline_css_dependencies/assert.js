const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const outputFiles = fs.readdirSync(outputDir);
const js = outputFiles
  .filter((file) => file.endsWith(".js"))
  .map((file) => fs.readFileSync(path.join(outputDir, file), "utf8"))
  .join("\n");

assert.doesNotMatch(
  outputFiles.join("\n"),
  /\.css$/,
  "inline CSS must not be emitted as standalone dependency stylesheets",
);
assert.match(
  js,
  /\.third-party-global\s*\{/,
  "global CSS imported by a dependency must be inlined",
);
assert.match(
  js,
  /\._loadingItem_fixture_1:before/,
  "precompiled dependency CSS must be inlined",
);
