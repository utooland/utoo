const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const externalNamespace = require("../../../helpers/external-namespace");

global.JSZip = function JSZip() {};
global.JSZip.version = "3.10.1";

async function evaluateExternal(factory) {
  let namespace;

  await factory({
    a(evaluate) {
      return new Promise((resolve, reject) => {
        evaluate(undefined, (error) => {
          if (error) {
            reject(error);
          } else {
            resolve();
          }
        });
      });
    },
    N: externalNamespace,
    n(value) {
      namespace = value;
    },
    S() {
      throw new Error("script loader should not run for an existing global");
    },
  });

  return namespace;
}

async function main() {
  const outputDir = path.join(__dirname, "output");
  const chunkFile = fs
    .readdirSync(outputDir)
    .find((file) => file.startsWith("_externals__") && file.endsWith(".js"));

  assert.ok(chunkFile, "expected a chunk containing the script external");

  globalThis.TURBOPACK = [];
  require(path.join(outputDir, chunkFile));

  const registration = globalThis.TURBOPACK.find((item) =>
    item.some(
      (value) =>
        typeof value === "string" &&
        value.includes("[external]") &&
        value.endsWith(", script)"),
    ),
  );
  assert.ok(registration, "expected a script external registration");

  const factory = registration.find(
    (value, index) =>
      typeof value === "function" &&
      typeof registration[index - 1] === "string" &&
      registration[index - 1].endsWith(", script)"),
  );
  const namespace = await evaluateExternal(factory);

  assert.strictEqual(namespace.default, global.JSZip);
  assert.strictEqual(namespace.version, "3.10.1");
  assert.strictEqual(namespace.__esModule, true);
  assert.strictEqual(namespace[Symbol.toStringTag], "Module");

  const webpackGetDefault = (module) =>
    module && module.__esModule ? () => module.default : () => module;
  assert.doesNotThrow(() => new (webpackGetDefault(namespace)())());
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
