const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { build } = require("@utoo/pack");

async function main() {
  const output = path.join(__dirname, "dist", "polyfill-regression");
  await build(
    {
      tracing: false,
      config: {
        mode: "production",
        persistentCaching: false,
        nodePolyfill: true,
        entry: [
          {
            name: "main",
            import: "./polyfill-regression/index.js",
            library: { name: "ExternalPolyfillRegression" },
          },
        ],
        output: { path: output, clean: true },
        externals: {
          "has-tostringtag": {
            root: "ExternalStringTag",
            subPath: {
              rules: [{ regex: "^/shams$", target: "shams" }],
            },
          },
        },
        sourceMaps: false,
        optimization: { minify: false, moduleIds: "named" },
      },
    },
    __dirname,
    process.env.UTOO_TEST_ROOT || path.resolve(__dirname, "../.."),
  );

  let externalCalls = 0;
  const context = vm.createContext({
    console,
    URL,
    process: { env: {} },
    ExternalStringTag: {
      shams() {
        externalCalls += 1;
        return "configured-package-shams";
      },
    },
  });
  const bundle = path.join(output, "main.js");
  vm.runInContext(fs.readFileSync(bundle, "utf8"), context, { filename: bundle });
  console.log(`external calls during polyfill initialization: ${externalCalls}`);
  assert.equal(
    externalCalls,
    0,
    "project externals.subPath rules must not replace embedded polyfill dependencies",
  );
  assert.equal(context.ExternalPolyfillRegression.checkArguments(), true);
  assert.equal(externalCalls, 0, "using the embedded polyfill must stay isolated");
  assert.equal(
    context.ExternalPolyfillRegression.configuredTagValue(),
    "configured-package-shams",
    "project package subpaths must still use the configured external",
  );
  assert.equal(externalCalls, 1);
  console.log("verified project subpath externals and embedded polyfill isolation");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
