const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const acorn = require("acorn");

const code = fs.readFileSync("output/main.js", "utf8");
acorn.parse(code, { ecmaVersion: 5, sourceType: "script" });
assert.deepEqual(
  fs
    .readdirSync("output", { recursive: true })
    .filter((file) => file.endsWith(".js")),
  ["main.js"],
);

async function main() {
  const document = {
    currentScript: { src: "https://example.test/main.js" },
    getElementsByTagName: () => [{ src: "https://example.test/main.js" }],
  };
  for (const host of [
    { document },
    { document: { ...document, currentScript: null } },
    { importScripts() {}, location: new URL("https://example.test/main.js") },
    { document, module: { exports: {} } },
  ]) {
    const context = {
      console,
      URL,
      ExternalValue: { answer: 17 },
      globalThis: undefined,
      ...host,
    };
    if (host.module) {
      context.global = context;
      context.exports = host.module.exports;
    } else {
      context.self = context;
    }
    Object.defineProperty(context, "globalThis", {
      value: undefined,
      writable: false,
    });
    const initialGlobals = Object.keys(context);
    vm.runInNewContext(code, context, { filename: "main.js" });
    assert.deepEqual(
      Object.keys(context).filter((key) => !initialGlobals.includes(key)),
      host.module ? [] : ["LegacyLibrary"],
    );
    const library = host.module ? host.module.exports : context.LegacyLibrary;
    assert.equal(context.globalThis, undefined);
    assert.equal(library.external, context.ExternalValue);
    const globals = library.globals("local self", "local alias");
    assert.equal(globals[0].ExternalValue, context.ExternalValue);
    assert.equal(globals[1], "local self");
    assert.equal(globals[2], "local alias");
    assert.equal(globals[3].globalThis, globals[0]);
    const local = library.localGlobal(23);
    assert.equal(local.globalThis, 23);
    assert.equal(local.property, 7);
    assert.equal(library.read(), 42);
    assert.equal(library.read({ answer: 7 }), 7);
    assert.equal(library.flag, 1);
    assert.equal(library.last({ next: { value: 1, next: { value: 2 } } }), 2);
    assert.equal(await library.load(), 43);
    assert.match(library.asset, /^https:\/\/example\.test\/.*\.svg$/);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
