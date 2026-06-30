import _types from "ext:utoo_rt_ext/node/util_types";

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
  if (superCtor === undefined || superCtor === null) {
    throw new TypeError("The super constructor to \"inherits\" must be not null, got " + superCtor + " (ctor: " + (ctor && ctor.name) + ")");
  }
  if (ctor === undefined || ctor === null) {
    throw new TypeError("The constructor to \"inherits\" must be not null");
  }
  if (superCtor.prototype === undefined) {
    throw new TypeError("The super constructor to \"inherits\" has no prototype (superCtor: " + superCtor.name + ")");
  }
  Object.defineProperty(ctor, "super_", { value: superCtor, writable: true, configurable: true });
  Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
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
  if (typeof fn === 'function') {
    Object.setPrototypeOf(promisified, fn);
  }
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

// Node 22's util.getCallSites(). Returns plain objects describing the current
// call stack. Implemented via Error.prepareStackTrace + captureStackTrace using
// only the CallSite methods this runtime exposes (getScriptHash is not one of
// them, which is why libraries like @eggjs/tegg's StackUtil prefer this API).
function getCallSites(frameCountOrOptions, maybeOptions) {
  let frameCount = 10;
  let sourceMap = false;
  if (typeof frameCountOrOptions === "number") {
    frameCount = frameCountOrOptions;
    if (maybeOptions && typeof maybeOptions === "object") {
      sourceMap = !!maybeOptions.sourceMap;
    }
  } else if (frameCountOrOptions && typeof frameCountOrOptions === "object") {
    if (typeof frameCountOrOptions.frameCount === "number") {
      frameCount = frameCountOrOptions.frameCount;
    }
    sourceMap = !!frameCountOrOptions.sourceMap;
  }
  const origPrepare = Error.prepareStackTrace;
  const origLimit = Error.stackTraceLimit;
  Error.prepareStackTrace = (_, stack) => stack;
  Error.stackTraceLimit = frameCount + 1;
  const holder = {};
  Error.captureStackTrace(holder, getCallSites);
  const stack = Array.isArray(holder.stack) ? holder.stack : [];
  Error.prepareStackTrace = origPrepare;
  Error.stackTraceLimit = origLimit;
  const call = (cs, name) => {
    try {
      return typeof cs[name] === "function" ? cs[name]() : undefined;
    } catch {
      return undefined;
    }
  };
  return stack.slice(0, frameCount).map((cs) => ({
    functionName: call(cs, "getFunctionName") ?? "",
    scriptName: call(cs, "getFileName") ?? "",
    scriptId: String(call(cs, "getScriptNameOrSourceURL") ?? ""),
    lineNumber: call(cs, "getLineNumber") ?? 0,
    columnNumber: call(cs, "getColumnNumber") ?? 0,
    column: call(cs, "getColumnNumber") ?? 0,
  }));
}

const types = _types;

const util = {
  format, inspect, inherits, deprecate, promisify, callbackify,
  debuglog, getCallSites, types,
  TextEncoder: globalThis.TextEncoder,
  TextDecoder: globalThis.TextDecoder,
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
  debuglog, getCallSites, types,
};
