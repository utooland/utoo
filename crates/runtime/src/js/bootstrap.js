// Bootstrap: wire up global APIs from native ops.
const __bootstrapOps = Deno.core.ops;

function __formatArgs(args) {
  return args
    .map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
    .join(" ");
}

globalThis.console = {
  log(...args) {
    __bootstrapOps.op_console_log(__formatArgs(args));
  },
  warn(...args) {
    __bootstrapOps.op_console_warn(__formatArgs(args));
  },
  error(...args) {
    __bootstrapOps.op_console_error(__formatArgs(args));
  },
  info(...args) {
    __bootstrapOps.op_console_log(__formatArgs(args));
  },
  debug(...args) {
    __bootstrapOps.op_console_log(__formatArgs(args));
  },
};

if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(str) {
      if (!str) return new Uint8Array(0);
      const bytes = [];
      for (let i = 0; i < str.length; i++) {
        let code = str.charCodeAt(i);
        if (code >= 0xd800 && code <= 0xdbff && i + 1 < str.length) {
          const next = str.charCodeAt(i + 1);
          if (next >= 0xdc00 && next <= 0xdfff) {
            code = ((code - 0xd800) << 10) + (next - 0xdc00) + 0x10000;
            i++;
          }
        }
        if (code < 0x80) bytes.push(code);
        else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
        else if (code < 0x10000) bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
        else bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
      }
      return new Uint8Array(bytes);
    }
    encodeInto(str, dest) {
      const encoded = this.encode(str);
      dest.set(encoded.subarray(0, dest.length));
      return { read: str.length, written: Math.min(encoded.length, dest.length) };
    }
    get encoding() { return "utf-8"; }
  };
}

if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class TextDecoder {
    constructor(encoding) { this._encoding = encoding || "utf-8"; }
    decode(buf) {
      if (!buf || buf.length === 0) return "";
      if (!(buf instanceof Uint8Array)) buf = new Uint8Array(buf.buffer || buf);
      let str = "", i = 0;
      while (i < buf.length) {
        const b = buf[i];
        if (b < 0x80) { str += String.fromCharCode(b); i++; }
        else if ((b & 0xe0) === 0xc0) { str += String.fromCharCode(((b & 0x1f) << 6) | (buf[i+1] & 0x3f)); i += 2; }
        else if ((b & 0xf0) === 0xe0) { str += String.fromCharCode(((b & 0x0f) << 12) | ((buf[i+1] & 0x3f) << 6) | (buf[i+2] & 0x3f)); i += 3; }
        else { const cp = ((b & 0x07) << 18) | ((buf[i+1] & 0x3f) << 12) | ((buf[i+2] & 0x3f) << 6) | (buf[i+3] & 0x3f); cp > 0xffff ? (str += String.fromCodePoint(cp)) : (str += String.fromCharCode(cp)); i += 4; }
      }
      return str;
    }
    get encoding() { return this._encoding; }
  };
}

// Timers - wire up to deno_core's built-in timer system
const __core = Deno.core;

globalThis.setTimeout = function setTimeout(cb, delay, ...args) {
  delay = Math.max(0, (delay | 0) || 0);
  const task = args.length > 0 ? () => cb(...args) : cb;
  return __core.queueUserTimer(
    __core.getTimerDepth() + 1,
    false,
    delay,
    task,
  );
};

globalThis.setInterval = function setInterval(cb, delay, ...args) {
  delay = Math.max(0, (delay | 0) || 0);
  const task = args.length > 0 ? () => cb(...args) : cb;
  return __core.queueUserTimer(
    __core.getTimerDepth() + 1,
    true,
    delay,
    task,
  );
};

globalThis.clearTimeout = function clearTimeout(id) {
  if (id != null) __core.cancelTimer(id);
};

globalThis.clearInterval = function clearInterval(id) {
  if (id != null) __core.cancelTimer(id);
};

// setImmediate / clearImmediate (Node.js compat)
globalThis.setImmediate = function setImmediate(cb, ...args) {
  return globalThis.setTimeout(cb, 0, ...args);
};

globalThis.clearImmediate = function clearImmediate(id) {
  globalThis.clearTimeout(id);
};

// queueMicrotask - V8 provides this, but ensure it's available
if (typeof globalThis.queueMicrotask === "undefined") {
  globalThis.queueMicrotask = function queueMicrotask(cb) {
    Promise.resolve().then(cb);
  };
}

// process.nextTick - uses deno_core's nextTick callback system
const __nextTickQueue = [];

__core.setNextTickCallback(() => {
  while (__nextTickQueue.length > 0) {
    const entry = __nextTickQueue.shift();
    entry[0](...entry.slice(1));
  }
  __core.setHasTickScheduled(false);
});

globalThis.process = {
  exit(code = 0) {
    __bootstrapOps.op_exit(code);
  },
  cwd() {
    return __bootstrapOps.op_cwd();
  },
  env: __bootstrapOps.op_env_to_object(),
  argv: ["utoo-runtime"],
  version: "v22.0.0",
  versions: { node: "22.0.0" },
  platform: __bootstrapOps.op_os_platform(),
  nextTick(cb, ...args) {
    __nextTickQueue.push([cb, ...args]);
    __core.setHasTickScheduled(true);
  },
};
