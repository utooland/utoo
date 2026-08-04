const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const marker = "PAGE_A_GLOBAL_ONLY_3275";
const outputDir = path.join(__dirname, "output");
const markerAsset = fs
  .readdirSync(outputDir)
  .filter((asset) => asset.endsWith(".css"))
  .find((asset) => fs.readFileSync(path.join(outputDir, asset), "utf8").includes(marker));

assert(markerAsset, "missing page A global CSS marker in emitted CSS");

const pageA = fs.readFileSync(path.join(outputDir, "a.js"), "utf8");
const pageB = fs.readFileSync(path.join(outputDir, "b.js"), "utf8");

assert(pageA.includes(markerAsset), "page A must load its global CSS asset");
assert(!pageB.includes(markerAsset), "page B must not load page A global CSS asset");
