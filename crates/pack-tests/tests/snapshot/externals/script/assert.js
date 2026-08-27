const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const externalNamespace = require("../../../helpers/external-namespace");

global._ = { marker: "lodash" };
global.EsmScript = {
  default: "esm-script",
  named: "esm-named",
};
Object.defineProperty(global.EsmScript, "__esModule", { value: true });

async function evaluateExternal(factory) {
  let namespace;

  return factory({
    a(evaluate) {
      return new Promise((resolve, reject) => {
        evaluate(undefined, (error) => {
          if (error) {
            reject(error);
          } else {
            resolve(namespace);
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
}

async function main() {
  const outputDir = path.join(__dirname, "output");
  const chunkFiles = fs
    .readdirSync(outputDir)
    .filter(
      (file) => file.startsWith("_externals__") && file.endsWith(".js"),
    );

  assert.ok(chunkFiles.length > 0, "expected chunks containing script externals");

  globalThis.TURBOPACK = [];
  for (const chunkFile of chunkFiles) {
    require(path.join(outputDir, chunkFile));
  }

  const factories = [];
  for (const registration of globalThis.TURBOPACK) {
    for (let index = 1; index < registration.length; index += 2) {
      const id = registration[index];
      if (
        typeof id === "string" &&
        id.includes("[external]") &&
        id.endsWith(", script)")
      ) {
        factories.push(registration[index + 1]);
      }
    }
  }

  assert.strictEqual(factories.length, 2);

  const namespaces = await Promise.all(factories.map(evaluateExternal));
  const lodashNamespace = namespaces.find((value) => value.marker === "lodash");
  const esmNamespace = namespaces.find((value) => value.named === "esm-named");

  assert.strictEqual(lodashNamespace.default, global._);
  assert.strictEqual(esmNamespace.default, "esm-script");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
