const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const chunkFile = fs
  .readdirSync(outputDir)
  .find((file) => file.startsWith("_root-of-the-server___") && file.endsWith(".js"));

assert.ok(chunkFile, "expected an inline CSS output chunk");

const chunk = fs.readFileSync(path.join(outputDir, chunkFile), "utf8");
const cssModuleMarker =
  '"[project]/style/inline_css/input/index.less?modules [client] (css module)"';
const entryMarker =
  '"[project]/style/inline_css/input/index.js [client] (ecmascript)"';
const cssModuleStart = chunk.indexOf(cssModuleMarker);
const entryStart = chunk.indexOf(entryMarker, cssModuleStart);

assert.notEqual(cssModuleStart, -1, "expected the CSS Modules facade");
assert.notEqual(entryStart, -1, "expected the JavaScript entry after the CSS Modules facade");

const cssModuleFactory = chunk.slice(cssModuleStart, entryStart);

assert.match(
  cssModuleFactory,
  /__turbopack_context__\.i\([^)]*index\.less\.css\.js\?modules/,
  "CSS Modules facade must evaluate the inline style injection module",
);
