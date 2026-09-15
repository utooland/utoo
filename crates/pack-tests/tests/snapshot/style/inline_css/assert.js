const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const chunkFile = fs
  .readdirSync(outputDir)
  .find(
    (file) =>
      file.endsWith(".js") &&
      !file.startsWith("turbopack-") &&
      fs
        .readFileSync(path.join(outputDir, file), "utf8")
        .includes("index.less?modules [client] (css module)"),
  );

assert.ok(chunkFile, "expected an inline CSS output chunk");

const chunk = fs.readFileSync(path.join(outputDir, chunkFile), "utf8");
const cssModuleMarker =
  '"[project]/style/inline_css/input/index.less?modules [client] (css module)"';
const entryMarker =
  '"[project]/style/inline_css/input/index.js [client] (ecmascript)"';
// Match factory definitions (marker at line start followed by the factory),
// not import calls that mention the same module id inside other factories.
const cssModuleStart = chunk.indexOf(`\n${cssModuleMarker},`);
const entryStart = chunk.indexOf(`\n${entryMarker},`);

assert.notEqual(cssModuleStart, -1, "expected the CSS Modules facade");
assert.notEqual(entryStart, -1, "expected the JavaScript entry");

// Strict factories are grouped separately from non-strict ones, so the entry
// may precede the facade. Slice the facade's own factory up to the next one.
const nextFactoryStart = chunk.indexOf(
  '\n"[project]/',
  cssModuleStart + cssModuleMarker.length,
);
const cssModuleFactory = chunk.slice(
  cssModuleStart,
  nextFactoryStart === -1 ? undefined : nextFactoryStart,
);

assert.match(
  cssModuleFactory,
  /__turbopack_context__\.i\([^)]*index\.less\.css\?modules/,
  "CSS Modules facade must evaluate the inline style injection module",
);
