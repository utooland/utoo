// Bootstrap: wire up global APIs from native ops.
const __bootstrapOps = Deno.core.ops;

// ---- Web Platform API polyfills (must be before ESM module init) ----

// DOMException
if (!globalThis.DOMException) {
  globalThis.DOMException = class DOMException extends Error {
    constructor(message, name) {
      super(message);
      this.name = name || "Error";
      this.code = 0;
    }
  };
}

// Blob
if (!globalThis.Blob) {
  globalThis.Blob = class Blob {
    constructor(parts, opts) {
      this._parts = parts || [];
      this.type = (opts && opts.type) || "";
      this.size = this._parts.reduce((acc, p) => acc + (p.length || p.byteLength || 0), 0);
    }
    async text() {
      return this._parts.map(p => typeof p === "string" ? p : new TextDecoder().decode(p)).join("");
    }
    async arrayBuffer() {
      const t = await this.text();
      return new TextEncoder().encode(t).buffer;
    }
    slice(start, end, type) { return new Blob([]); }
    stream() { return null; }
  };
}

// File
if (!globalThis.File) {
  globalThis.File = class File extends globalThis.Blob {
    constructor(parts, name, opts) {
      super(parts, opts);
      this.name = name;
      this.lastModified = (opts && opts.lastModified) || Date.now();
    }
  };
}

// AbortController/AbortSignal
if (!globalThis.AbortController) {
  class AbortSignal {
    constructor() { this.aborted = false; this.reason = undefined; this._listeners = []; }
    addEventListener(type, fn) { this._listeners.push(fn); }
    removeEventListener(type, fn) { this._listeners = this._listeners.filter(l => l !== fn); }
    throwIfAborted() { if (this.aborted) throw this.reason; }
    static abort(reason) {
      const s = new AbortSignal();
      s.aborted = true; s.reason = reason || new DOMException("signal is aborted without reason");
      return s;
    }
    static timeout(ms) {
      const s = new AbortSignal();
      globalThis.setTimeout(() => {
        s.aborted = true; s.reason = new DOMException("signal timed out");
        for (const fn of s._listeners) fn();
      }, ms);
      return s;
    }
  }
  globalThis.AbortSignal = AbortSignal;
  globalThis.AbortController = class AbortController {
    constructor() { this.signal = new AbortSignal(); }
    abort(reason) {
      this.signal.aborted = true;
      this.signal.reason = reason || new DOMException("The operation was aborted");
      for (const fn of this.signal._listeners) fn();
    }
  };
}

// performance (minimal Web Performance API)
if (!globalThis.performance) {
  const __timeOrigin = Date.now();
  globalThis.performance = {
    now() { return Date.now() - __timeOrigin; },
    timeOrigin: __timeOrigin,
    mark() {},
    measure() {},
    clearMarks() {},
    clearMeasures() {},
    getEntries() { return []; },
    getEntriesByName() { return []; },
    getEntriesByType() { return []; },
    toJSON() { return { timeOrigin: __timeOrigin }; },
  };
}

// structuredClone
if (!globalThis.structuredClone) {
  globalThis.structuredClone = function structuredClone(value) {
    return JSON.parse(JSON.stringify(value));
  };
}

// EventTarget / Event polyfill
if (!globalThis.EventTarget) {
  globalThis.EventTarget = class EventTarget {
    constructor() { this._listeners = {}; }
    addEventListener(type, cb, opts) {
      if (!this._listeners[type]) this._listeners[type] = [];
      this._listeners[type].push({ cb, once: opts && opts.once });
    }
    removeEventListener(type, cb) {
      if (!this._listeners[type]) return;
      this._listeners[type] = this._listeners[type].filter(l => l.cb !== cb);
    }
    dispatchEvent(event) {
      const listeners = this._listeners[event.type];
      if (!listeners) return true;
      for (const l of [...listeners]) {
        l.cb.call(this, event);
        if (l.once) this.removeEventListener(event.type, l.cb);
      }
      return !event.defaultPrevented;
    }
  };
}

if (!globalThis.Event) {
  globalThis.Event = class Event {
    constructor(type, opts) {
      this.type = type;
      this.bubbles = (opts && opts.bubbles) || false;
      this.cancelable = (opts && opts.cancelable) || false;
      this.composed = (opts && opts.composed) || false;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.timeStamp = Date.now();
      this.isTrusted = false;
    }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    stopPropagation() {}
    stopImmediatePropagation() {}
    composedPath() { return []; }
  };
}

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent extends globalThis.Event {
    constructor(type, opts) {
      super(type, opts);
      this.detail = (opts && opts.detail) || null;
    }
  };
}

// ---- End Web Platform API polyfills ----

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

// Node.js compat: global === globalThis
globalThis.global = globalThis;

// Minimal stdio streams for Node.js compat
const __stdout = {
  fd: 1,
  isTTY: false,
  columns: 80,
  rows: 24,
  write(data) {
    if (typeof data === "string") __bootstrapOps.op_console_log(data.replace(/\n$/, ""));
    return true;
  },
  on() { return this; },
  once() { return this; },
  emit() { return false; },
  end() {},
  destroy() {},
  writable: true,
  _isStdio: true,
};

const __stderr = {
  fd: 2,
  isTTY: false,
  columns: 80,
  rows: 24,
  write(data) {
    if (typeof data === "string") __bootstrapOps.op_console_error(data.replace(/\n$/, ""));
    return true;
  },
  on() { return this; },
  once() { return this; },
  emit() { return false; },
  end() {},
  destroy() {},
  writable: true,
  _isStdio: true,
};

const __stdin = {
  fd: 0,
  isTTY: false,
  readable: true,
  on() { return this; },
  once() { return this; },
  emit() { return false; },
  pause() { return this; },
  resume() { return this; },
  read() { return null; },
  destroy() {},
  _isStdio: true,
};

globalThis.process = {
  exit(code = 0) {
    __bootstrapOps.op_exit(code);
  },
  cwd() {
    return __bootstrapOps.op_cwd();
  },
  env: __bootstrapOps.op_env_to_object(),
  argv: ["utoo-runtime"],
  execArgv: [],
  execPath: "utoo-runtime",
  pid: 1,
  ppid: 0,
  title: "utoo-runtime",
  version: "v22.0.0",
  versions: { node: "22.0.0" },
  platform: __bootstrapOps.op_os_platform(),
  arch: __bootstrapOps.op_os_arch(),
  release: { name: "node" },
  config: {},
  features: {},
  stdout: __stdout,
  stderr: __stderr,
  stdin: __stdin,
  hrtime(time) {
    const now = Date.now();
    const sec = Math.floor(now / 1000);
    const nano = (now % 1000) * 1e6;
    if (time) {
      return [sec - time[0], nano - time[1]];
    }
    return [sec, nano];
  },
  memoryUsage() {
    return { rss: 0, heapTotal: 0, heapUsed: 0, external: 0, arrayBuffers: 0 };
  },
  cpuUsage() {
    return { user: 0, system: 0 };
  },
  uptime() { return 0; },
  on() { return this; },
  once() { return this; },
  off() { return this; },
  emit() { return false; },
  removeListener() { return this; },
  removeAllListeners() { return this; },
  listeners() { return []; },
  addListener() { return this; },
  prependListener() { return this; },
  prependOnceListener() { return this; },
  listenerCount() { return 0; },
  binding(name) {
    throw new Error(`process.binding('${name}') is not supported in utoo-runtime`);
  },
  _linkedBinding(name) {
    throw new Error(`process._linkedBinding('${name}') is not supported in utoo-runtime`);
  },
  umask() { return 0o22; },
  nextTick(cb, ...args) {
    __nextTickQueue.push([cb, ...args]);
    __core.setHasTickScheduled(true);
  },
};
