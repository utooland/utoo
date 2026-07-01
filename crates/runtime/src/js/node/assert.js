class AssertionError extends Error {
  constructor(opts) {
    const msg = typeof opts === "string" ? opts : opts.message || "Assertion failed";
    super(msg);
    this.name = "AssertionError";
    this.code = "ERR_ASSERTION";
    if (typeof opts === "object") {
      this.actual = opts.actual;
      this.expected = opts.expected;
      this.operator = opts.operator;
    }
  }
}

function fail(msg) {
  throw new AssertionError(msg || "Failed");
}

function ok(value, msg) {
  if (!value) throw new AssertionError(msg || `Expected truthy value, got ${value}`);
}

function equal(actual, expected, msg) {
  if (actual != expected) {
    throw new AssertionError({ message: msg || `${actual} != ${expected}`, actual, expected, operator: "==" });
  }
}

function notEqual(actual, expected, msg) {
  if (actual == expected) {
    throw new AssertionError({ message: msg || `${actual} == ${expected}`, actual, expected, operator: "!=" });
  }
}

function strictEqual(actual, expected, msg) {
  if (actual !== expected) {
    throw new AssertionError({ message: msg || `${actual} !== ${expected}`, actual, expected, operator: "===" });
  }
}

function notStrictEqual(actual, expected, msg) {
  if (actual === expected) {
    throw new AssertionError({ message: msg || `${actual} === ${expected}`, actual, expected, operator: "!==" });
  }
}

function _deepEqual(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return false;
  if (typeof a !== "object") return false;
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    return a.every((v, i) => _deepEqual(v, b[i]));
  }
  const ka = Object.keys(a).sort();
  const kb = Object.keys(b).sort();
  if (ka.length !== kb.length) return false;
  return ka.every((k, i) => k === kb[i] && _deepEqual(a[k], b[k]));
}

function deepEqual(actual, expected, msg) {
  if (!_deepEqual(actual, expected)) {
    throw new AssertionError({ message: msg || "deepEqual failed", actual, expected, operator: "deepEqual" });
  }
}

function deepStrictEqual(actual, expected, msg) {
  if (!_deepEqual(actual, expected)) {
    throw new AssertionError({ message: msg || "deepStrictEqual failed", actual, expected, operator: "deepStrictEqual" });
  }
}

function throws(fn, expected, msg) {
  let threw = false;
  try { fn(); } catch { threw = true; }
  if (!threw) throw new AssertionError(msg || "Missing expected exception");
}

function doesNotThrow(fn, msg) {
  try { fn(); } catch (e) {
    throw new AssertionError(msg || `Got unwanted exception: ${e.message}`);
  }
}

function ifError(err) {
  if (err) throw err;
}

const assert = ok;
Object.assign(assert, {
  ok, equal, notEqual, strictEqual, notStrictEqual,
  deepEqual, deepStrictEqual, throws, doesNotThrow,
  fail, ifError, AssertionError,
});
assert.default = assert;
assert.strict = assert;

const strict = assert;

export default assert;
export {
  ok, equal, notEqual, strictEqual, notStrictEqual,
  deepEqual, deepStrictEqual, throws, doesNotThrow,
  fail, ifError, AssertionError, strict,
};
