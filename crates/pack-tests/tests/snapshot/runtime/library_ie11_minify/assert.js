const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const acorn = require("acorn");

const code = fs.readFileSync("output/main.js", "utf8");
acorn.parse(code, { ecmaVersion: 5, sourceType: "script" });
assert.deepEqual(fs.readdirSync("output"), ["main.js"]);

const browser = {
  Symbol: undefined,
  URL: undefined,
  Proxy: undefined,
  dynamicKey: "dynamicValue",
  functionKey: "functionValue",
  primitiveSource: "hi",
};
const commonJs = {
  ...browser,
  module: { exports: {} },
  exports: {},
};
for (const context of [browser, commonJs]) {
  if (context.module) context.global = context;
  else context.self = context;
  vm.createContext(context);
  vm.runInContext("Array.prototype.includes = undefined", context);
  vm.runInContext(code, context, { filename: "main.js" });
  const library = context.module
    ? context.module.exports
    : context.Ie11Library;
  assert.equal(library.legacy.value, 23);
  assert.equal(library.dynamicValue, 17);
  assert.equal(library.computed, 17);
  assert.equal(library.inherited, 5);
  assert.equal("hidden" in library, false);
  assert.equal(library.staticValue, "explicit");
  assert.equal(library.functionValue, 19);
  assert.equal(library[0], "h");
  assert.equal(library[1], "i");
  assert.equal("dynamicValue" in library, true);
  assert.equal(Object.keys(library).includes("dynamicValue"), true);
  assert.equal(Object.prototype.hasOwnProperty.call(library, "default"), false);
  const descriptor = Object.getOwnPropertyDescriptor(library, "dynamicValue");
  assert.equal(typeof descriptor.get, "function");
  assert.equal(descriptor.enumerable, true);
  assert.equal(descriptor.configurable, false);
  library.updateDynamic();
  assert.equal(library.dynamicValue, 42);
  assert.equal(library.computed, 42);
  assert.equal(library.dynamic.addedLater, 99);
  assert.equal("addedLater" in library, false);

  const reads = [];
  function node(name, value, next) {
    return Object.defineProperty({ value }, "next", {
      get() {
        reads.push(name);
        return next;
      },
    });
  }
  const tail = node("tail", 2, null);
  const middle = node("middle", 1, tail);
  const head = node("head", 0, middle);
  assert.equal(library.last(head), 2);
  assert.deepEqual(reads, ["head", "middle", "middle", "tail"]);

  const read = library.makeReader(42);
  assert.equal(read.call({ value: 7 }), 42);

  const collect = (values) => Array.from(library.collect(values));
  assert.deepEqual(collect([1, 2, 3]), [1, 2, 3]);
  let evaluations = 0;
  assert.deepEqual(
    Array.from(library.collectReturned(() => {
      evaluations++;
      return [1, 2, 3];
    })),
    [1, 2, 3],
  );
  assert.equal(evaluations, 1);
  assert.deepEqual(collect("a😀"), ["a", "\ud83d", "\ude00"]);
  assert.deepEqual(collect(new Uint8Array([4, 5])), [4, 5]);
  assert.deepEqual(collect((function () { return arguments; })(6, 7)), [6, 7]);
  assert.deepEqual(collect(new Set([8, 9])), [8, 9]);
  assert.throws(() => library.collect({ 0: 1, length: 1 }), /non-iterable/);
  assert.throws(() => library.explicitIterator([]), /iterator/);

  const calls = [];
  const iterable = Object.defineProperty({}, "@@iterator", {
    get() {
      calls.push("get");
      return function () {
        assert.equal(this, iterable);
        calls.push("start");
        return {
          next() { return { value: 11, done: false }; },
          return() { calls.push("return"); return { done: true }; },
        };
      };
    },
  });
  assert.equal(library.first(iterable), 11);
  assert.deepEqual(calls, ["get", "start", "return"]);
  calls.length = 0;
  const error = new Error("body failure");
  assert.throws(() => library.throwFromLoop(iterable, error), (thrown) => thrown === error);
  assert.deepEqual(calls, ["get", "start", "return"]);

  // Real iterators still take precedence, including array overrides.
  context.Symbol = Symbol;
  const overridden = [1, 2, 3];
  overridden[Symbol.iterator] = function () {
    let done = false;
    return { next() { const step = { value: 13, done }; done = true; return step; } };
  };
  assert.deepEqual(collect(overridden), [13]);
}
