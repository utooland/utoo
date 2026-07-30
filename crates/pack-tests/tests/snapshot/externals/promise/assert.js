const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

global.$ = { ready: true };

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
    n(value) {
      namespace = value;
    },
  });
}

async function main() {
  const outputDir = path.join(__dirname, "output");
  const chunkFile = fs
    .readdirSync(outputDir)
    .find(
      (file) =>
        file.startsWith("_root-of-the-server___") && file.endsWith(".js"),
    );

  assert.ok(chunkFile, "expected a root chunk containing promise externals");

  globalThis.TURBOPACK = [];
  require(path.join(outputDir, chunkFile));

  const registration = globalThis.TURBOPACK.at(-1);
  const factories = [];

  for (let index = 1; index < registration.length; index += 2) {
    const id = registration[index];
    if (typeof id === "string" && id.includes("[external] (promise)")) {
      factories.push(registration[index + 1]);
    }
  }

  assert.strictEqual(factories.length, 4);

  const namespaces = await Promise.all(factories.map(evaluateExternal));
  const commonJsNamespace = namespaces.find((value) => value.named === true);
  const esmNamespace = namespaces.find((value) => value.named === "esm-named");
  const nativeEsmNamespace = namespaces.find(
    (value) => value.named === "native-esm-named",
  );

  assert.deepStrictEqual(commonJsNamespace.default, {
    default: "async-value",
    named: true,
  });
  assert.strictEqual(esmNamespace.default, "async-esm-value");
  assert.strictEqual(typeof nativeEsmNamespace.default, "function");
  assert.strictEqual(nativeEsmNamespace.__esModule, true);
  assert.strictEqual(nativeEsmNamespace[Symbol.toStringTag], "Module");
  assert.doesNotThrow(() => new nativeEsmNamespace.default());
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
