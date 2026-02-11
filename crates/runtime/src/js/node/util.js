function format(...args) {
  if (args.length === 0) return "";
  const first = args[0];
  if (typeof first !== "string") return args.map((a) => inspect(a)).join(" ");
  let i = 1;
  let str = first.replace(/%[sdjoO%]/g, (match) => {
    if (match === "%%") return "%";
    if (i >= args.length) return match;
    const arg = args[i++];
    switch (match) {
      case "%s": return String(arg);
      case "%d": return Number(arg).toString();
      case "%j": try { return JSON.stringify(arg); } catch { return "[Circular]"; }
      case "%o": case "%O": return inspect(arg);
      default: return match;
    }
  });
  while (i < args.length) str += " " + inspect(args[i++]);
  return str;
}

function inspect(obj, opts) {
  if (obj === null) return "null";
  if (obj === undefined) return "undefined";
  if (typeof obj === "string") return `'${obj}'`;
  if (typeof obj === "number" || typeof obj === "boolean") return String(obj);
  if (typeof obj === "function") return `[Function: ${obj.name || "anonymous"}]`;
  if (typeof obj === "symbol") return obj.toString();
  if (obj instanceof Date) return obj.toISOString();
  if (obj instanceof RegExp) return obj.toString();
  if (obj instanceof Error) return obj.stack || obj.message;
  if (Array.isArray(obj)) {
    const items = obj.map((i) => inspect(i, opts));
    return `[ ${items.join(", ")} ]`;
  }
  if (typeof obj === "object") {
    if (typeof obj[inspect.custom] === "function") {
      return obj[inspect.custom](opts && opts.depth, opts);
    }
    const keys = Object.keys(obj);
    if (keys.length === 0) return "{}";
    const items = keys.map((k) => `${k}: ${inspect(obj[k], opts)}`);
    return `{ ${items.join(", ")} }`;
  }
  return String(obj);
}
inspect.custom = Symbol.for("nodejs.util.inspect.custom");

function inherits(ctor, superCtor) {
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
  ctor.super_ = superCtor;
}

function deprecate(fn, msg) {
  let warned = false;
  function deprecated(...args) {
    if (!warned) {
      console.warn("DeprecationWarning:", msg);
      warned = true;
    }
    return fn.apply(this, args);
  }
  Object.setPrototypeOf(deprecated, fn);
  return deprecated;
}

function promisify(fn) {
  function promisified(...args) {
    return new Promise((resolve, reject) => {
      fn.call(this, ...args, (err, ...vals) => {
        if (err) return reject(err);
        resolve(vals.length <= 1 ? vals[0] : vals);
      });
    });
  }
  Object.setPrototypeOf(promisified, fn);
  return promisified;
}

function callbackify(fn) {
  return function (...args) {
    const cb = args.pop();
    fn.apply(this, args).then(
      (val) => cb(null, val),
      (err) => cb(err instanceof Error ? err : new Error(String(err))),
    );
  };
}

function debuglog() {
  return function () {};
}

const types = {
  isDate: (v) => v instanceof Date,
  isRegExp: (v) => v instanceof RegExp,
  isPromise: (v) => v instanceof Promise,
  isArray: Array.isArray,
  isTypedArray: (v) => ArrayBuffer.isView(v) && !(v instanceof DataView),
  isUint8Array: (v) => v instanceof Uint8Array,
  isArrayBuffer: (v) => v instanceof ArrayBuffer,
  isNativeError: (v) => v instanceof Error,
  isSet: (v) => v instanceof Set,
  isMap: (v) => v instanceof Map,
};

const util = {
  format, inspect, inherits, deprecate, promisify, callbackify,
  debuglog, types,
  isArray: Array.isArray,
  isBuffer: (obj) => obj instanceof Uint8Array && obj.constructor.name === "Buffer",
  isFunction: (v) => typeof v === "function",
  isString: (v) => typeof v === "string",
  isNumber: (v) => typeof v === "number",
  isObject: (v) => typeof v === "object" && v !== null,
  isUndefined: (v) => v === undefined,
  isNull: (v) => v === null,
  isNullOrUndefined: (v) => v == null,
  isPrimitive: (v) => v === null || (typeof v !== "object" && typeof v !== "function"),
  _extend: (target, source) => Object.assign(target, source),
};

export default util;
export {
  format, inspect, inherits, deprecate, promisify, callbackify,
  debuglog, types,
};
