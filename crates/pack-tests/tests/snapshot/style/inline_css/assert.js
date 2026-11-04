const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

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
const context = vm.createContext({ TURBOPACK: [] });
vm.runInContext(chunk, context, { filename: chunkFile });
// Strict factories may be grouped in a nested array. Inspect registrations
// without depending on the generated chunk's whitespace or wrapper layout.
const factories = context.TURBOPACK[0].slice(1).flat();
const cssModuleIndex = factories.indexOf(
  "[project]/style/inline_css/input/index.less?modules [client] (css module)",
);
assert.notEqual(cssModuleIndex, -1, "expected the CSS Modules facade");
assert.ok(
  factories.includes(
    "[project]/style/inline_css/input/index.js [client] (ecmascript)",
  ),
  "expected the JavaScript entry",
);
const cssModuleFactory = factories[cssModuleIndex + 1];
assert.equal(typeof cssModuleFactory, "function");
const imports = [];
cssModuleFactory({
  i(moduleId) {
    imports.push(moduleId);
  },
  v() {},
});
assert.ok(
  imports.some((moduleId) =>
    /index\.less\.css\?modules.*\(inline css, ecmascript\)/.test(moduleId),
  ),
  "CSS Modules facade must evaluate the inline style injection module",
);
