const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const externalNamespace = require("../../../helpers/external-namespace");

const nativeEsmUrl =
  "data:text/javascript;base64,ZXhwb3J0IGRlZmF1bHQgY2xhc3MgTmF0aXZlRXNtIHt9OyBleHBvcnQgY29uc3QgbmFtZWQgPSAnbmF0aXZlLWVzbS1uYW1lZCc7IGV4cG9ydCBsZXQgY291bnQgPSAwOyBleHBvcnQgZnVuY3Rpb24gaW5jcmVtZW50KCkgeyBjb3VudCArPSAxOyB9";

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
    async y() {
      return import(nativeEsmUrl);
    },
    N: externalNamespace,
    n(value) {
      namespace = value;
    },
  });

  return namespace;
}

async function main() {
  const outputDir = path.join(__dirname, "output");

  globalThis.TURBOPACK = [];
  for (const file of fs.readdirSync(outputDir)) {
    if (file !== "main.js" && file.endsWith(".js")) {
      require(path.join(outputDir, file));
    }
  }

  let factory;
  for (const registration of globalThis.TURBOPACK) {
    for (let index = 1; index < registration.length; index += 2) {
      const id = registration[index];
      if (
        typeof id === "string" &&
        id.includes("[external]") &&
        id.endsWith(", esm_import)")
      ) {
        factory = registration[index + 1];
      }
    }
  }

  assert.ok(factory, "expected an ESM import external registration");

  const namespace = await evaluateExternal(factory);

  assert.strictEqual(typeof namespace.default, "function");
  assert.strictEqual(namespace.named, "native-esm-named");
  assert.strictEqual(namespace.__esModule, true);
  assert.strictEqual(namespace[Symbol.toStringTag], "Module");
  assert.doesNotThrow(() => new namespace.default());
  assert.strictEqual(namespace.count, 0);
  namespace.increment();
  assert.strictEqual(namespace.count, 1);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
