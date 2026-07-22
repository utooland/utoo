const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const library = require("./output/main.js");

assert.equal(library.staticBasename("/tmp/example.txt"), "example.txt");
assert.equal(library.load("node:path"), path);
assert.equal(library.loadAgain("node:path"), path);
assert.equal(library.load("__proto__"), path);

assert.throws(
  () => library.load("node:fs"),
  (err) => {
    assert.equal(err.code, "MODULE_NOT_FOUND");
    assert.match(err.message, /not declared as a CommonJS external/);
    return true;
  },
);

assert.throws(
  () => library.load("global-only"),
  (err) => {
    assert.equal(err.code, "MODULE_NOT_FOUND");
    assert.match(err.message, /not declared as a CommonJS external/);
    return true;
  },
);

const hook = Symbol.for("@utoo/pack/runtime-require");
let resolvedRequest;
globalThis[hook] = (request) => {
  resolvedRequest = request;
  return { fromHook: request };
};

assert.deepEqual(library.load("path-alias"), { fromHook: "node:path" });
assert.equal(resolvedRequest, "node:path");

delete globalThis[hook];

const output = fs.readFileSync("./output/main.js", "utf8");
assert.equal(
  (output.match(/\["node:path"\]: "node:path"/g) || []).length,
  1,
  "the runtime external map should be emitted once per module",
);
