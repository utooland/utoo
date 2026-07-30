const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const output = fs.readFileSync(path.join(__dirname, "output/main.js"), "utf8");

const helperStart = output.indexOf("function externalNamespace");
const helperAssignment = "contextPrototype.N = externalNamespace;";
const helperEnd = output.indexOf(helperAssignment, helperStart);

assert.notStrictEqual(helperStart, -1);
assert.notStrictEqual(helperEnd, -1);
assert.match(
  output,
  /__turbopack_context__\.n\(__turbopack_context__\.N\(mod\)\)/,
);

const context = {
  contextPrototype: {},
  createGetter: (object, key) => () => object[key],
  toStringTag: Symbol.toStringTag,
};
vm.runInNewContext(
  output.slice(helperStart, helperEnd + helperAssignment.length),
  context,
);

const externalValue = { count: 0 };
const namespace = context.contextPrototype.N(externalValue);

assert.strictEqual(namespace.default, externalValue);
assert.strictEqual(namespace.count, 0);
externalValue.count = 1;
assert.strictEqual(namespace.count, 1);
assert.strictEqual(namespace.__esModule, true);
assert.strictEqual(namespace[Symbol.toStringTag], "Module");

const esmValue = { default: "initial", count: 0 };
const esmNamespace = Object.create(null);
Object.defineProperty(esmNamespace, Symbol.toStringTag, { value: "Module" });
for (const key of ["default", "count"]) {
  Object.defineProperty(esmNamespace, key, {
    enumerable: true,
    get: () => esmValue[key],
  });
}

const normalizedEsmNamespace = context.contextPrototype.N(esmNamespace);
assert.strictEqual(normalizedEsmNamespace.default, "initial");
assert.strictEqual(normalizedEsmNamespace.count, 0);
esmValue.default = "updated";
esmValue.count = 1;
assert.strictEqual(normalizedEsmNamespace.default, "updated");
assert.strictEqual(normalizedEsmNamespace.count, 1);
