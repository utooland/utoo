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

assert.equal(
  javascriptAssets.filter(({ content }) =>
    /source:\s*["'](?:entry|dynamic)["']/.test(content),
  ).length,
  0,
  "hot=false must not emit chunk-list bootstraps that wait for an HMR client",
);

const asyncLoader = javascriptAssets.find(({ content }) =>
  content.includes("return parentImport("),
);
assert.ok(asyncLoader, "the fixture should emit an async loader");
const loaderMatch = asyncLoader.content.match(
  /return Promise\.all\((\[[\s\S]*?\])\.map/,
);
assert.ok(loaderMatch, "the async loader should contain a chunk array");
assert.ok(
  JSON.parse(loaderMatch[1]).length > 0,
  "hot=false must load dynamic content chunks directly",
);
