const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const acorn = require("acorn");

const code = fs.readFileSync(path.join(__dirname, "output/main.js"), "utf8");
acorn.parse(code, { ecmaVersion: 6, sourceType: "script" });
const externalValue = { answer: 42 };
const scripts = [];
const context = { console, URL };
context.self = context;
Object.defineProperty(context, "globalThis", {
  value: undefined,
  writable: false,
});
context.document = {
  currentScript: { src: "https://example.test/main.js" },
  createElement(name) {
    assert.equal(name, "script");
    return {};
  },
  head: {
    appendChild(script) {
      assert.equal(script.src, "https://example.test/external.js");
      scripts.push(script);
      context.ScriptValue = externalValue;
      queueMicrotask(() => script.onload());
    },
  },
};

async function main() {
  vm.runInNewContext(code, context, { filename: "main.js" });
  const library = context.ScriptReexport;
  assert.equal(library.value, 17);
  assert.equal(scripts.length, 0);

  const results = await Promise.all([
    library.loadExternal(),
    library.loadExternal(),
  ]);
  assert.equal(results[0], externalValue);
  assert.equal(results[1], externalValue);
  assert.equal(await library.loadExternal(), externalValue);
  assert.equal(scripts.length, 1);
  assert.equal(library.value, 17);
  assert.equal(context.globalThis, undefined);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
