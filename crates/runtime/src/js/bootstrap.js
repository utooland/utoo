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

// atob / btoa (Base64)
if (!globalThis.atob) {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
  globalThis.atob = function atob(str) {
    str = String(str).replace(/=+$/, "");
    let output = "";
    for (let i = 0; i < str.length; ) {
      const a = chars.indexOf(str.charAt(i++));
      const b = chars.indexOf(str.charAt(i++));
      const c = chars.indexOf(str.charAt(i++));
      const d = chars.indexOf(str.charAt(i++));
      const n = (a << 18) | (b << 12) | (c << 6) | d;
      output += String.fromCharCode((n >> 16) & 0xff);
      if (c !== 64) output += String.fromCharCode((n >> 8) & 0xff);
      if (d !== 64) output += String.fromCharCode(n & 0xff);
    }
    return output;
  };
  globalThis.btoa = function btoa(str) {
    let output = "";
    for (let i = 0; i < str.length; ) {
      const a = str.charCodeAt(i++);
      const b = i < str.length ? str.charCodeAt(i++) : NaN;
      const c = i < str.length ? str.charCodeAt(i++) : NaN;
      const n1 = a >> 2;
      const n2 = ((a & 3) << 4) | (b >> 4);
      const n3 = isNaN(b) ? 64 : ((b & 15) << 2) | (c >> 6);
      const n4 = isNaN(c) ? 64 : c & 63;
      output += chars[n1] + chars[n2] + chars[n3] + chars[n4];
    }
    return output;
  };
}

// TextEncoderStream / TextDecoderStream
if (!globalThis.TextEncoderStream) {
  globalThis.TextEncoderStream = class TextEncoderStream {
    constructor() {
      this.encoding = "utf-8";
      this.readable = null;
      this.writable = null;
    }
  };
}
if (!globalThis.TextDecoderStream) {
  globalThis.TextDecoderStream = class TextDecoderStream {
    constructor(encoding) {
      this.encoding = encoding || "utf-8";
      this.readable = null;
      this.writable = null;
    }
  };
}

// CryptoKey -- opaque key object for Web Crypto API
class CryptoKey {
  constructor(type, extractable, algorithm, usages, keyData) {
    this.type = type;
    this.extractable = extractable;
    this.algorithm = Object.freeze(algorithm);
    this.usages = Object.freeze(usages);
    // internal raw key bytes (not exposed via public API)
    Object.defineProperty(this, "_keyData", { value: keyData, enumerable: false });
  }
  get [Symbol.toStringTag]() { return "CryptoKey"; }
}
globalThis.CryptoKey = CryptoKey;

// SubtleCrypto -- Web Crypto API
function __toUint8Array(data) {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new Uint8Array(data);
}

const __subtle = {
  async generateKey(algorithm, extractable, keyUsages) {
    const algo = typeof algorithm === "string" ? { name: algorithm } : algorithm;
    const name = algo.name.toUpperCase();
    if (name === "AES-GCM" || name === "AES-CBC" || name === "AES-CTR") {
      const length = algo.length || 256;
      const keyBytes = new Uint8Array(__bootstrapOps.op_crypto_random_bytes(length / 8));
      return new CryptoKey("secret", extractable, { name: algo.name, length }, keyUsages, keyBytes);
    }
    if (name === "HMAC") {
      const hash = typeof algo.hash === "string" ? algo.hash : algo.hash.name;
      const length = algo.length || 256;
      const keyBytes = new Uint8Array(__bootstrapOps.op_crypto_random_bytes(length / 8));
      return new CryptoKey("secret", extractable, { name: "HMAC", hash: { name: hash }, length }, keyUsages, keyBytes);
    }
    throw new DOMException("Unsupported algorithm: " + algo.name, "NotSupportedError");
  },
  async importKey(format, keyData, algorithm, extractable, keyUsages) {
    const algo = typeof algorithm === "string" ? { name: algorithm } : algorithm;
    if (format === "raw") {
      const bytes = __toUint8Array(keyData);
      const name = algo.name.toUpperCase();
      if (name === "AES-GCM" || name === "AES-CBC" || name === "AES-CTR") {
        return new CryptoKey("secret", extractable, { name: algo.name, length: bytes.length * 8 }, keyUsages, new Uint8Array(bytes));
      }
      if (name === "HMAC") {
        const hash = typeof algo.hash === "string" ? algo.hash : algo.hash.name;
        return new CryptoKey("secret", extractable, { name: "HMAC", hash: { name: hash }, length: bytes.length * 8 }, keyUsages, new Uint8Array(bytes));
      }
    }
    throw new DOMException("Unsupported import: format=" + format + ", algorithm=" + algo.name, "NotSupportedError");
  },
  async exportKey(format, key) {
    if (!key.extractable) throw new DOMException("Key is not extractable", "InvalidAccessError");
    if (format === "raw") {
      return key._keyData.buffer.slice(0);
    }
    throw new DOMException("Unsupported export format: " + format, "NotSupportedError");
  },
  async encrypt(algorithm, key, data) {
    const algo = typeof algorithm === "string" ? { name: algorithm } : algorithm;
    const name = algo.name.toUpperCase();
    const plaintext = __toUint8Array(data);
    if (name === "AES-GCM") {
      const iv = __toUint8Array(algo.iv);
      const aad = algo.additionalData ? __toUint8Array(algo.additionalData) : new Uint8Array(0);
      const result = __bootstrapOps.op_crypto_aes_gcm_encrypt(key._keyData, iv, plaintext, aad);
      return new Uint8Array(result).buffer;
    }
    throw new DOMException("Unsupported encryption algorithm: " + algo.name, "NotSupportedError");
  },
  async decrypt(algorithm, key, data) {
    const algo = typeof algorithm === "string" ? { name: algorithm } : algorithm;
    const name = algo.name.toUpperCase();
    const ciphertext = __toUint8Array(data);
    if (name === "AES-GCM") {
      const iv = __toUint8Array(algo.iv);
      const aad = algo.additionalData ? __toUint8Array(algo.additionalData) : new Uint8Array(0);
      const result = __bootstrapOps.op_crypto_aes_gcm_decrypt(key._keyData, iv, ciphertext, aad);
      return new Uint8Array(result).buffer;
    }
    throw new DOMException("Unsupported decryption algorithm: " + algo.name, "NotSupportedError");
  },
  async digest(algorithm, data) {
    const algoName = typeof algorithm === "string" ? algorithm : algorithm.name;
    const bytes = __toUint8Array(data);
    const rid = __bootstrapOps.op_crypto_hash_create(algoName);
    __bootstrapOps.op_crypto_hash_update(rid, bytes);
    const result = __bootstrapOps.op_crypto_hash_digest(rid);
    return new Uint8Array(result).buffer;
  },
  async sign(algorithm, key, data) {
    const algo = typeof algorithm === "string" ? { name: algorithm } : algorithm;
    const name = algo.name.toUpperCase();
    const bytes = __toUint8Array(data);
    if (name === "HMAC") {
      const hash = key.algorithm.hash.name;
      const rid = __bootstrapOps.op_crypto_hmac_create(hash, key._keyData);
      __bootstrapOps.op_crypto_hmac_update(rid, bytes);
      const result = __bootstrapOps.op_crypto_hmac_digest(rid);
      return new Uint8Array(result).buffer;
    }
    throw new DOMException("Unsupported sign algorithm: " + algo.name, "NotSupportedError");
  },
  async verify(algorithm, key, signature, data) {
    const signResult = await this.sign(algorithm, key, data);
    const sig1 = new Uint8Array(signature);
    const sig2 = new Uint8Array(signResult);
    if (sig1.length !== sig2.length) return false;
    let result = 0;
    for (let i = 0; i < sig1.length; i++) result |= sig1[i] ^ sig2[i];
    return result === 0;
  },
  async deriveBits() { throw new DOMException("deriveBits not supported", "NotSupportedError"); },
  async deriveKey() { throw new DOMException("deriveKey not supported", "NotSupportedError"); },
  async wrapKey() { throw new DOMException("wrapKey not supported", "NotSupportedError"); },
  async unwrapKey() { throw new DOMException("unwrapKey not supported", "NotSupportedError"); },
};

// crypto.getRandomValues / crypto.randomUUID / crypto.subtle
if (!globalThis.crypto) {
  globalThis.crypto = {
    getRandomValues(arr) {
      for (let i = 0; i < arr.length; i++) {
        arr[i] = Math.floor(Math.random() * 256);
      }
      return arr;
    },
    randomUUID() {
      const bytes = new Uint8Array(16);
      globalThis.crypto.getRandomValues(bytes);
      bytes[6] = (bytes[6] & 0x0f) | 0x40;
      bytes[8] = (bytes[8] & 0x3f) | 0x80;
      const hex = Array.from(bytes).map(b => b.toString(16).padStart(2, "0")).join("");
      return hex.slice(0,8) + "-" + hex.slice(8,12) + "-" + hex.slice(12,16) + "-" + hex.slice(16,20) + "-" + hex.slice(20);
    },
    subtle: __subtle,
  };
} else if (!globalThis.crypto.getRandomValues) {
  globalThis.crypto.getRandomValues = function(arr) {
    for (let i = 0; i < arr.length; i++) arr[i] = Math.floor(Math.random() * 256);
    return arr;
  };
}
if (globalThis.crypto && !globalThis.crypto.subtle) {
  globalThis.crypto.subtle = __subtle;
}

// Headers
if (!globalThis.Headers) {
  globalThis.Headers = class Headers {
    constructor(init) {
      this._map = new Map();
      if (init) {
        if (init instanceof Headers) {
          init.forEach((v, k) => this.set(k, v));
        } else if (Array.isArray(init)) {
          for (const [k, v] of init) this.set(k, v);
        } else if (typeof init === "object") {
          for (const k of Object.keys(init)) this.set(k, init[k]);
        }
      }
    }
    append(name, value) {
      const key = name.toLowerCase();
      const existing = this._map.get(key);
      this._map.set(key, existing ? existing + ", " + value : String(value));
    }
    delete(name) { this._map.delete(name.toLowerCase()); }
    get(name) { return this._map.get(name.toLowerCase()) || null; }
    has(name) { return this._map.has(name.toLowerCase()); }
    set(name, value) { this._map.set(name.toLowerCase(), String(value)); }
    forEach(cb, thisArg) { this._map.forEach((v, k) => cb.call(thisArg, v, k, this)); }
    entries() { return this._map.entries(); }
    keys() { return this._map.keys(); }
    values() { return this._map.values(); }
    [Symbol.iterator]() { return this._map.entries(); }
  };
}

// Request
if (!globalThis.Request) {
  globalThis.Request = class Request {
    constructor(input, init) {
      if (input instanceof Request) {
        this.url = input.url;
        this.method = input.method;
        this.headers = new Headers(input.headers);
        this._body = input._body;
      } else {
        this.url = String(input);
        this.method = (init && init.method) || "GET";
        this.headers = new Headers(init && init.headers);
        this._body = init && init.body;
      }
      if (init) {
        if (init.method) this.method = init.method;
        if (init.headers) this.headers = new Headers(init.headers);
        if (init.body !== undefined) this._body = init.body;
      }
      this.redirect = (init && init.redirect) || "follow";
      this.signal = (init && init.signal) || null;
      this.cache = (init && init.cache) || "default";
      this.credentials = (init && init.credentials) || "same-origin";
      this.mode = (init && init.mode) || "cors";
      this.referrer = (init && init.referrer) || "about:client";
      this.bodyUsed = false;
      this.destination = "";
      this.integrity = "";
      this.keepalive = false;
    }
    clone() { return new Request(this); }
    async text() { this.bodyUsed = true; return typeof this._body === "string" ? this._body : ""; }
    async json() { return JSON.parse(await this.text()); }
    async arrayBuffer() {
      this.bodyUsed = true;
      if (this._body instanceof ArrayBuffer) return this._body;
      return new TextEncoder().encode(await this.text()).buffer;
    }
    async blob() { return new Blob([await this.arrayBuffer()]); }
    async formData() { throw new Error("formData() not implemented"); }
  };
}

// Response
if (!globalThis.Response) {
  globalThis.Response = class Response {
    constructor(body, init) {
      this._body = body;
      this.status = (init && init.status) || 200;
      this.statusText = (init && init.statusText) || "";
      this.headers = new Headers(init && init.headers);
      this.ok = this.status >= 200 && this.status < 300;
      this.type = "basic";
      this.url = "";
      this.redirected = false;
      this.bodyUsed = false;
    }
    clone() { return new Response(this._body, { status: this.status, statusText: this.statusText, headers: this.headers }); }
    async text() {
      this.bodyUsed = true;
      if (typeof this._body === "string") return this._body;
      if (this._body instanceof ArrayBuffer) return new TextDecoder().decode(this._body);
      if (this._body instanceof Uint8Array) return new TextDecoder().decode(this._body);
      return this._body ? String(this._body) : "";
    }
    async json() { return JSON.parse(await this.text()); }
    async arrayBuffer() {
      this.bodyUsed = true;
      if (this._body instanceof ArrayBuffer) return this._body;
      return new TextEncoder().encode(await this.text()).buffer;
    }
    async blob() { return new Blob([await this.arrayBuffer()]); }
    async formData() { throw new Error("formData() not implemented"); }
    static json(data, init) {
      const body = JSON.stringify(data);
      const headers = new Headers(init && init.headers);
      if (!headers.has("content-type")) headers.set("content-type", "application/json");
      return new Response(body, { ...init, headers });
    }
    static redirect(url, status) {
      return new Response(null, { status: status || 302, headers: { Location: url } });
    }
    static error() { return new Response(null, { status: 0 }); }
  };
}

// Web Streams API
if (!globalThis.ReadableStream) {
  globalThis.ReadableStream = class ReadableStream {
    constructor(underlyingSource, strategy) {
      this._source = underlyingSource;
      this._controller = null;
      this._locked = false;
      this._disturbed = false;
      this._state = "readable";
      this._reader = null;
      this._storedError = undefined;
      this._queue = [];
      this._closeRequested = false;
      // Pending read resolvers -- fulfilled when enqueue/close/error is called
      this._pendingReads = [];
      this._pulling = false;
      if (underlyingSource) {
        const controller = {
          enqueue: (chunk) => {
            if (this._pendingReads.length > 0) {
              const resolve = this._pendingReads.shift();
              resolve({ value: chunk, done: false });
            } else {
              this._queue.push(chunk);
            }
            this._pulling = false;
          },
          close: () => {
            this._closeRequested = true;
            this._state = "closed";
            while (this._pendingReads.length > 0) {
              const resolve = this._pendingReads.shift();
              resolve({ value: undefined, done: true });
            }
          },
          error: (e) => {
            this._state = "errored";
            this._storedError = e;
            while (this._pendingReads.length > 0) {
              const reject = this._pendingReads.shift();
              reject({ value: undefined, done: true });
            }
          },
          get desiredSize() { return 1; },
        };
        this._controller = controller;
        if (underlyingSource.start) {
          try {
            const startResult = underlyingSource.start(controller);
            // If start returns a promise, handle it
            if (startResult && typeof startResult.then === "function") {
              startResult.catch((e) => controller.error(e));
            }
          } catch(e) { controller.error(e); }
        }
      }
    }
    get locked() { return this._locked; }
    cancel() {
      this._state = "closed";
      while (this._pendingReads.length > 0) {
        const resolve = this._pendingReads.shift();
        resolve({ value: undefined, done: true });
      }
      return Promise.resolve();
    }
    getReader() {
      this._locked = true;
      const stream = this;
      const reader = {
        read() {
          stream._disturbed = true;
          // Return queued data first
          if (stream._queue.length > 0) {
            return Promise.resolve({ value: stream._queue.shift(), done: false });
          }
          // Stream closed or errored
          if (stream._closeRequested || stream._state === "closed") {
            return Promise.resolve({ value: undefined, done: true });
          }
          if (stream._state === "errored") {
            return Promise.reject(stream._storedError);
          }
          // Queue is empty, stream still open -- wait for data
          return new Promise((resolve) => {
            stream._pendingReads.push(resolve);
            // Call pull if the source has one and we're not already pulling
            if (stream._source && stream._source.pull && !stream._pulling) {
              stream._pulling = true;
              try {
                const pullResult = stream._source.pull(stream._controller);
                if (pullResult && typeof pullResult.then === "function") {
                  pullResult.catch((e) => stream._controller.error(e));
                }
              } catch(e) {
                stream._controller.error(e);
              }
            }
          });
        },
        releaseLock() { stream._locked = false; stream._reader = null; },
        cancel() { return stream.cancel(); },
        get closed() {
          if (stream._state === "closed" || stream._closeRequested) {
            return Promise.resolve();
          }
          return new Promise(() => {}); // never resolves until close
        },
      };
      stream._reader = reader;
      return reader;
    }
    tee() {
      const reader = this.getReader();
      const s1 = new ReadableStream({
        async pull(controller) {
          const { value, done } = await reader.read();
          if (done) { controller.close(); return; }
          controller.enqueue(value);
        }
      });
      return [s1, s1]; // Simplified: both branches read same data
    }
    pipeThrough(transform) {
      const reader = this.getReader();
      const writer = transform.writable.getWriter();
      (async () => {
        while (true) {
          const { value, done } = await reader.read();
          if (done) { await writer.close(); break; }
          await writer.write(value);
        }
      })().catch(e => writer.abort(e));
      return transform.readable;
    }
    pipeTo(writable) {
      const reader = this.getReader();
      const writer = writable.getWriter();
      return (async () => {
        while (true) {
          const { value, done } = await reader.read();
          if (done) { await writer.close(); break; }
          await writer.write(value);
        }
      })();
    }
    [Symbol.asyncIterator]() {
      const reader = this.getReader();
      return {
        next() { return reader.read(); },
        return() { reader.releaseLock(); return Promise.resolve({ value: undefined, done: true }); },
      };
    }
    static from(iterable) {
      return new ReadableStream({
        async start(controller) {
          for await (const chunk of iterable) {
            controller.enqueue(chunk);
          }
          controller.close();
        }
      });
    }
  };
}

if (!globalThis.WritableStream) {
  globalThis.WritableStream = class WritableStream {
    constructor(underlyingSink, strategy) {
      this._sink = underlyingSink;
      this._locked = false;
      this._state = "writable";
    }
    get locked() { return this._locked; }
    abort() { return Promise.resolve(); }
    close() {
      if (this._sink && this._sink.close) this._sink.close();
      this._state = "closed";
      return Promise.resolve();
    }
    getWriter() {
      this._locked = true;
      const stream = this;
      return {
        write(chunk) {
          if (stream._sink && stream._sink.write) return Promise.resolve(stream._sink.write(chunk));
          return Promise.resolve();
        },
        close() { return stream.close(); },
        abort() { return Promise.resolve(); },
        releaseLock() { stream._locked = false; },
        get ready() { return Promise.resolve(); },
        get closed() { return Promise.resolve(); },
        get desiredSize() { return 1; },
      };
    }
  };
}

if (!globalThis.TransformStream) {
  globalThis.TransformStream = class TransformStream {
    constructor(transformer) {
      this.readable = new ReadableStream();
      this.writable = new WritableStream();
    }
  };
}

if (!globalThis.ByteLengthQueuingStrategy) {
  globalThis.ByteLengthQueuingStrategy = class ByteLengthQueuingStrategy {
    constructor(init) { this.highWaterMark = init.highWaterMark; }
    size(chunk) { return chunk.byteLength; }
  };
}

if (!globalThis.CountQueuingStrategy) {
  globalThis.CountQueuingStrategy = class CountQueuingStrategy {
    constructor(init) { this.highWaterMark = init.highWaterMark; }
    size() { return 1; }
  };
}

// fetch (minimal - will throw for actual network requests, but allows type checks)
if (!globalThis.fetch) {
  globalThis.fetch = async function fetch(input, init) {
    throw new Error("fetch() is not fully implemented in utoo-runtime");
  };
}

// ---- End Web Platform API polyfills ----

function __formatArgs(args) {
  return args
    .map((a) => (typeof a === "string" ? a : JSON.stringify(a)))
    .join(" ");
}

globalThis.console = {
  log(...args) { __bootstrapOps.op_console_log(__formatArgs(args)); },
  warn(...args) { __bootstrapOps.op_console_warn(__formatArgs(args)); },
  error(...args) { __bootstrapOps.op_console_error(__formatArgs(args)); },
  info(...args) { __bootstrapOps.op_console_log(__formatArgs(args)); },
  debug(...args) { __bootstrapOps.op_console_log(__formatArgs(args)); },
  trace(...args) { __bootstrapOps.op_console_log("Trace: " + __formatArgs(args)); },
  assert(condition, ...args) {
    if (!condition) __bootstrapOps.op_console_error("Assertion failed: " + __formatArgs(args));
  },
  dir(obj) { __bootstrapOps.op_console_log(__formatArgs([obj])); },
  dirxml(obj) { __bootstrapOps.op_console_log(__formatArgs([obj])); },
  table(data) { __bootstrapOps.op_console_log(__formatArgs([data])); },
  clear() {},
  count() {},
  countReset() {},
  group() {},
  groupCollapsed() {},
  groupEnd() {},
  time() {},
  timeEnd() {},
  timeLog() {},
  timeStamp() {},
  profile() {},
  profileEnd() {},
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
// Wrap callbacks to preserve async context (AsyncLocalStorage) across macrotasks.
// V8's ContinuationPreservedEmbedderData only covers microtasks (Promise/await),
// so we must manually capture and restore context for setTimeout/setInterval/nextTick.
const __core = Deno.core;
const __getAsyncContext = __core.getAsyncContext;
const __setAsyncContext = __core.setAsyncContext;

globalThis.setTimeout = function setTimeout(cb, delay, ...args) {
  delay = Math.max(0, (delay | 0) || 0);
  const ctx = __getAsyncContext();
  const task = () => {
    const prev = __setAsyncContext(ctx);
    try {
      return args.length > 0 ? cb(...args) : cb();
    } finally {
      __setAsyncContext(prev);
    }
  };
  return __core.queueUserTimer(
    __core.getTimerDepth() + 1,
    false,
    delay,
    task,
  );
};

globalThis.setInterval = function setInterval(cb, delay, ...args) {
  delay = Math.max(0, (delay | 0) || 0);
  const ctx = __getAsyncContext();
  const task = () => {
    const prev = __setAsyncContext(ctx);
    try {
      return args.length > 0 ? cb(...args) : cb();
    } finally {
      __setAsyncContext(prev);
    }
  };
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
    const [ctx, fn, ...args] = entry;
    const prev = __setAsyncContext(ctx);
    try {
      fn(...args);
    } finally {
      __setAsyncContext(prev);
    }
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
  hrtime: Object.assign(function hrtime(time) {
    const now = Date.now();
    const sec = Math.floor(now / 1000);
    const nano = (now % 1000) * 1e6;
    if (time) {
      return [sec - time[0], nano - time[1]];
    }
    return [sec, nano];
  }, {
    bigint() {
      return BigInt(Math.floor(performance.now() * 1e6));
    },
  }),
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
    // Return stubs for common bindings to avoid hard crashes
    if (name === "fs") return {};
    if (name === "constants") return {};
    if (name === "natives") return {};
    if (name === "buffer") return {};
    return {};
  },
  _linkedBinding(name) {
    return {};
  },
  umask() { return 0o22; },
  nextTick(cb, ...args) {
    __nextTickQueue.push([__getAsyncContext(), cb, ...args]);
    __core.setHasTickScheduled(true);
  },
};
