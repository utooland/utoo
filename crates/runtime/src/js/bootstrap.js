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
  var __timeOrigin = 0;
  globalThis.performance = {
    now() { if (!__timeOrigin) __timeOrigin = Date.now(); return Date.now() - __timeOrigin; },
    get timeOrigin() { if (!__timeOrigin) __timeOrigin = Date.now(); return __timeOrigin; },
    mark() {},
    measure() {},
    clearMarks() {},
    clearMeasures() {},
    getEntries() { return []; },
    getEntriesByName() { return []; },
    getEntriesByType() { return []; },
    toJSON() { if (!__timeOrigin) __timeOrigin = Date.now(); return { timeOrigin: __timeOrigin }; },
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
      var canceled1 = false;
      var canceled2 = false;
      var reason1, reason2;
      var branch1Controller, branch2Controller;
      var cancelResolve;
      var cancelPromise = new Promise(function (r) { cancelResolve = r; });
      var reading = false;
      var readAgain = false;

      function pullAlgorithm() {
        if (reading) { readAgain = true; return Promise.resolve(); }
        reading = true;
        return reader.read().then(function (result) {
          reading = false;
          var done = result.done;
          var value = result.value;
          if (done) {
            if (!canceled1 && branch1Controller) branch1Controller.close();
            if (!canceled2 && branch2Controller) branch2Controller.close();
            return;
          }
          // Clone value for each branch (shallow copy for typed arrays)
          var v1 = value;
          var v2 = value;
          if (!canceled1 && !canceled2 && value && typeof value.slice === "function") {
            v2 = value.slice(0);
          }
          if (!canceled1 && branch1Controller) branch1Controller.enqueue(v1);
          if (!canceled2 && branch2Controller) branch2Controller.enqueue(v2);
          if (readAgain) { readAgain = false; return pullAlgorithm(); }
        });
      }

      var branch1 = new ReadableStream({
        start: function (c) { branch1Controller = c; },
        pull: pullAlgorithm,
        cancel: function (reason) {
          canceled1 = true;
          reason1 = reason;
          if (canceled2) { cancelResolve(); }
          return cancelPromise;
        }
      });
      var branch2 = new ReadableStream({
        start: function (c) { branch2Controller = c; },
        pull: pullAlgorithm,
        cancel: function (reason) {
          canceled2 = true;
          reason2 = reason;
          if (canceled1) { cancelResolve(); }
          return cancelPromise;
        }
      });
      return [branch1, branch2];
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
    constructor(transformer, writableStrategy, readableStrategy) {
      const xform = transformer || {};
      let readableController;
      const self = this;

      this.readable = new ReadableStream({
        start(controller) {
          readableController = controller;
        },
        cancel(reason) {
          // If the readable side is cancelled, abort the writable side
          if (self._writableController) {
            self._writableController.error(reason);
          }
        }
      }, readableStrategy);

      this.writable = new WritableStream({
        start(controller) {
          self._writableController = controller;
        },
        async write(chunk) {
          if (xform.transform) {
            await xform.transform(chunk, {
              enqueue(c) { readableController.enqueue(c); },
              error(e) { readableController.error(e); },
              terminate() { readableController.close(); }
            });
          } else {
            // Identity transform -- pass through
            readableController.enqueue(chunk);
          }
        },
        async close() {
          if (xform.flush) {
            await xform.flush({
              enqueue(c) { readableController.enqueue(c); },
              error(e) { readableController.error(e); },
              terminate() { readableController.close(); }
            });
          }
          readableController.close();
        },
        abort(reason) {
          readableController.error(reason);
        }
      }, writableStrategy);
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
  if (args.length === 0) return "";
  var first = args[0];
  if (typeof first === "string" && args.length > 1 && /%[sdjoO%]/.test(first)) {
    var i = 1;
    var str = first.replace(/%[sdjoO%]/g, function(m) {
      if (m === "%%") return "%";
      if (i >= args.length) return m;
      var a = args[i++];
      if (m === "%s") return String(a);
      if (m === "%d") return Number(a).toString();
      if (m === "%j" || m === "%o" || m === "%O") try { return JSON.stringify(a); } catch(e) { return "[Circular]"; }
      return m;
    });
    while (i < args.length) { str += " " + (typeof args[i] === "string" ? args[i] : JSON.stringify(args[i])); i++; }
    return str;
  }
  return args
    .map(function(a) { return typeof a === "string" ? a : JSON.stringify(a); })
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
      let read = 0;
      let written = 0;
      for (let i = 0; i < str.length; i++) {
        let code = str.charCodeAt(i);
        let byteLen;
        if (code >= 0xd800 && code <= 0xdbff && i + 1 < str.length) {
          const next = str.charCodeAt(i + 1);
          if (next >= 0xdc00 && next <= 0xdfff) {
            code = ((code - 0xd800) << 10) + (next - 0xdc00) + 0x10000;
          }
        }
        if (code < 0x80) byteLen = 1;
        else if (code < 0x800) byteLen = 2;
        else if (code < 0x10000) byteLen = 3;
        else byteLen = 4;
        if (written + byteLen > dest.length) break;
        if (code < 0x80) {
          dest[written++] = code;
        } else if (code < 0x800) {
          dest[written++] = 0xc0 | (code >> 6);
          dest[written++] = 0x80 | (code & 0x3f);
        } else if (code < 0x10000) {
          dest[written++] = 0xe0 | (code >> 12);
          dest[written++] = 0x80 | ((code >> 6) & 0x3f);
          dest[written++] = 0x80 | (code & 0x3f);
        } else {
          dest[written++] = 0xf0 | (code >> 18);
          dest[written++] = 0x80 | ((code >> 12) & 0x3f);
          dest[written++] = 0x80 | ((code >> 6) & 0x3f);
          dest[written++] = 0x80 | (code & 0x3f);
          i++; // skip the low surrogate
        }
        read = i + 1;
      }
      return { read: read, written: written };
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

// Node.js setTimeout/setInterval return Timeout objects with ref/unref methods.
function __makeTimeout(id) {
  return {
    _id: id,
    ref: function() { return this; },
    unref: function() { return this; },
    hasRef: function() { return true; },
    refresh: function() { return this; },
    [Symbol.toPrimitive]: function() { return id; },
  };
}

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
  var id = __core.queueUserTimer(
    __core.getTimerDepth() + 1,
    false,
    delay,
    task,
  );
  return __makeTimeout(id);
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
  var id = __core.queueUserTimer(
    __core.getTimerDepth() + 1,
    true,
    delay,
    task,
  );
  return __makeTimeout(id);
};

globalThis.clearTimeout = function clearTimeout(id) {
  if (id != null) __core.cancelTimer(typeof id === "object" && id._id != null ? id._id : id);
};

globalThis.clearInterval = function clearInterval(id) {
  if (id != null) __core.cancelTimer(typeof id === "object" && id._id != null ? id._id : id);
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

// Exposed on globalThis so post-snapshot reinit can re-register it
// (setNextTickCallback stores in Rust ContextState, not V8 heap)
globalThis.__utoo_nextTickDrainer = () => {
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
};
__core.setNextTickCallback(globalThis.__utoo_nextTickDrainer);

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
  addListener() { return this; },
  removeListener() { return this; },
  removeAllListeners() { return this; },
  setMaxListeners() { return this; },
  getMaxListeners() { return 10; },
  listeners() { return []; },
  rawListeners() { return []; },
  listenerCount() { return 0; },
  eventNames() { return []; },
  prependListener() { return this; },
  prependOnceListener() { return this; },
  off() { return this; },
  pipe() { return this; },
  cork() {},
  uncork() {},
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
  addListener() { return this; },
  removeListener() { return this; },
  removeAllListeners() { return this; },
  setMaxListeners() { return this; },
  getMaxListeners() { return 10; },
  listeners() { return []; },
  rawListeners() { return []; },
  listenerCount() { return 0; },
  eventNames() { return []; },
  prependListener() { return this; },
  prependOnceListener() { return this; },
  off() { return this; },
  pipe() { return this; },
  cork() {},
  uncork() {},
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
  addListener() { return this; },
  removeListener() { return this; },
  removeAllListeners() { return this; },
  setMaxListeners() { return this; },
  getMaxListeners() { return 10; },
  listeners() { return []; },
  rawListeners() { return []; },
  listenerCount() { return 0; },
  eventNames() { return []; },
  prependListener() { return this; },
  prependOnceListener() { return this; },
  off() { return this; },
  pipe() { return this; },
  _isStdio: true,
};

// Global event target stubs (used by service workers / tegg)
var __globalEventListeners = {};
globalThis.addEventListener = function(type, fn) {
  if (!__globalEventListeners[type]) __globalEventListeners[type] = [];
  __globalEventListeners[type].push(fn);
};
globalThis.removeEventListener = function(type, fn) {
  var list = __globalEventListeners[type];
  if (!list) return;
  var idx = list.indexOf(fn);
  if (idx >= 0) list.splice(idx, 1);
};
globalThis.dispatchEvent = function(event) {
  var type = event && event.type ? event.type : event;
  var list = __globalEventListeners[type];
  if (!list) return true;
  for (var i = 0; i < list.length; i++) list[i](event);
  return true;
};

var __processListeners = {};

// Node-compat shim over the leaked `globalThis.Deno`. deno_core exposes
// `Deno.core` for internal ops, which trips libraries (has-flag, supports-color,
// chalk, etc.) that feature-detect `globalThis.Deno` and then use Deno-only APIs
// like Deno.args / Deno.env / Deno.build. Provide the small subset they need,
// backed by process, so they don't crash on this Node-ish runtime.
try {
  const D = globalThis.Deno;
  if (D && !D.args) {
    Object.defineProperty(D, "args", {
      configurable: true,
      get() {
        const a = globalThis.process && globalThis.process.argv;
        return Array.isArray(a) ? a.slice(2) : [];
      },
    });
    if (!D.env) {
      D.env = {
        get: (k) => globalThis.process.env[k],
        set: (k, v) => { globalThis.process.env[k] = String(v); },
        has: (k) => Object.prototype.hasOwnProperty.call(globalThis.process.env, k),
        delete: (k) => { delete globalThis.process.env[k]; },
        toObject: () => ({ ...globalThis.process.env }),
      };
    }
    if (!D.build) {
      Object.defineProperty(D, "build", {
        configurable: true,
        get() {
          return { os: globalThis.process.platform, arch: globalThis.process.arch, target: "", vendor: "unknown", env: "" };
        },
      });
    }
    if (!("noColor" in D)) {
      Object.defineProperty(D, "noColor", {
        configurable: true,
        get() { return !!globalThis.process.env.NO_COLOR; },
      });
    }
    if (!D.version) D.version = { deno: "0.0.0", v8: "11.3.244.8", typescript: "5.0.0" };
    if (!D.pid) Object.defineProperty(D, "pid", { configurable: true, get() { return globalThis.process.pid; } });
    if (typeof D.exit !== "function") D.exit = (code = 0) => globalThis.process.exit(code);
    if (typeof D.cwd !== "function") D.cwd = () => globalThis.process.cwd();
  }
} catch (_e) { /* ignore */ }

globalThis.process = {
  exit(code = 0) {
    __bootstrapOps.op_exit(code);
  },
  cwd() {
    return __bootstrapOps.op_cwd();
  },
  get env() {
    var v = __bootstrapOps.op_env_to_object();
    Object.defineProperty(this, "env", { value: v, writable: true, configurable: true, enumerable: true });
    return v;
  },
  set env(v) {
    Object.defineProperty(this, "env", { value: v, writable: true, configurable: true, enumerable: true });
  },
  argv: ["utoo-runtime"],
  execArgv: [],
  get execPath() {
    var v = __bootstrapOps.op_exec_path();
    Object.defineProperty(this, "execPath", { value: v, writable: true, configurable: true, enumerable: true });
    return v;
  },
  set execPath(v) {
    Object.defineProperty(this, "execPath", { value: v, writable: true, configurable: true, enumerable: true });
  },
  pid: 1,
  ppid: 0,
  title: "utoo-runtime",
  version: "v20.0.0",
  versions: { node: "20.0.0", modules: "115", v8: "11.3.244.8", uv: "1.44.2", openssl: "3.0.8", ares: "1.19.0", icu: "73.1", unicode: "15.0", napi: "9" },
  get platform() {
    var v = __bootstrapOps.op_os_platform();
    Object.defineProperty(this, "platform", { value: v, writable: false, configurable: true, enumerable: true });
    return v;
  },
  get arch() {
    var v = __bootstrapOps.op_os_arch();
    Object.defineProperty(this, "arch", { value: v, writable: false, configurable: true, enumerable: true });
    return v;
  },
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
  on(event, fn) {
    if (!__processListeners[event]) __processListeners[event] = [];
    __processListeners[event].push({ fn: fn, once: false });
    return this;
  },
  once(event, fn) {
    if (!__processListeners[event]) __processListeners[event] = [];
    __processListeners[event].push({ fn: fn, once: true });
    return this;
  },
  off(event, fn) {
    var list = __processListeners[event];
    if (!list) return this;
    __processListeners[event] = list.filter(function(l) { return l.fn !== fn; });
    return this;
  },
  emit(event) {
    var list = __processListeners[event];
    if (!list || list.length === 0) return false;
    var args = [];
    for (var i = 1; i < arguments.length; i++) args.push(arguments[i]);
    var snapshot = list.slice();
    for (var j = 0; j < snapshot.length; j++) {
      snapshot[j].fn.apply(this, args);
      if (snapshot[j].once) {
        var idx = list.indexOf(snapshot[j]);
        if (idx !== -1) list.splice(idx, 1);
      }
    }
    return true;
  },
  removeListener(event, fn) { return this.off(event, fn); },
  removeAllListeners(event) {
    if (event) { delete __processListeners[event]; } else { __processListeners = {}; }
    return this;
  },
  listeners(event) {
    var list = __processListeners[event];
    return list ? list.map(function(l) { return l.fn; }) : [];
  },
  addListener(event, fn) { return this.on(event, fn); },
  prependListener(event, fn) {
    if (!__processListeners[event]) __processListeners[event] = [];
    __processListeners[event].unshift({ fn: fn, once: false });
    return this;
  },
  prependOnceListener(event, fn) {
    if (!__processListeners[event]) __processListeners[event] = [];
    __processListeners[event].unshift({ fn: fn, once: true });
    return this;
  },
  listenerCount(event) {
    var list = __processListeners[event];
    return list ? list.length : 0;
  },
  eventNames() { return Object.keys(__processListeners); },
  rawListeners(event) {
    var list = __processListeners[event];
    return list ? list.map(function(l) { return l.fn; }) : [];
  },
  setMaxListeners() { return this; },
  getMaxListeners() { return 10; },
  binding(name) {
    // Return stubs for common bindings to avoid hard crashes
    if (name === "constants") {
      // Lazy-init and cache
      if (!this._bindingConstants) {
        // macOS (Darwin) POSIX constants
        var c = {
          O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 512, O_EXCL: 2048,
          O_NOCTTY: 131072, O_TRUNC: 1024, O_APPEND: 8, O_DIRECTORY: 1048576,
          O_NOFOLLOW: 256, O_SYNC: 128, O_NONBLOCK: 4,
          S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192,
          S_IFBLK: 24576, S_IFIFO: 4096, S_IFLNK: 40960, S_IFSOCK: 49152,
          S_IRWXU: 448, S_IRUSR: 256, S_IWUSR: 128, S_IXUSR: 64,
          S_IRWXG: 56, S_IRGRP: 32, S_IWGRP: 16, S_IXGRP: 8,
          S_IRWXO: 7, S_IROTH: 4, S_IWOTH: 2, S_IXOTH: 1,
          F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
          SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGILL: 4, SIGTRAP: 5,
          SIGABRT: 6, SIGFPE: 8, SIGKILL: 9, SIGBUS: 10, SIGSEGV: 11,
          SIGSYS: 12, SIGPIPE: 13, SIGALRM: 14, SIGTERM: 15, SIGURG: 16,
          SIGSTOP: 17, SIGTSTP: 18, SIGCONT: 19, SIGCHLD: 20, SIGTTIN: 21,
          SIGTTOU: 22, SIGIO: 23, SIGXCPU: 24, SIGXFSZ: 25, SIGVTALRM: 26,
          SIGPROF: 27, SIGWINCH: 28, SIGUSR1: 30, SIGUSR2: 31,
          E2BIG: 7, EACCES: 13, EADDRINUSE: 48, EADDRNOTAVAIL: 49,
          EAFNOSUPPORT: 47, EAGAIN: 35, EALREADY: 37, EBADF: 9,
          EBADMSG: 94, EBUSY: 16, ECANCELED: 89, ECHILD: 10,
          ECONNABORTED: 53, ECONNREFUSED: 61, ECONNRESET: 54, EDEADLK: 11,
          EDESTADDRREQ: 39, EDOM: 33, EDQUOT: 69, EEXIST: 17,
          EFAULT: 14, EFBIG: 27, EHOSTUNREACH: 65, EINPROGRESS: 36,
          EINTR: 4, EINVAL: 22, EIO: 5, EISCONN: 56, EISDIR: 21,
          ELOOP: 62, EMFILE: 24, EMLINK: 31, EMSGSIZE: 40,
          ENAMETOOLONG: 63, ENETDOWN: 50, ENETRESET: 52, ENETUNREACH: 51,
          ENFILE: 23, ENOBUFS: 55, ENODEV: 19, ENOENT: 2,
          ENOEXEC: 8, ENOLCK: 77, ENOMEM: 12, ENOPROTOOPT: 42,
          ENOSPC: 28, ENOSYS: 78, ENOTCONN: 57, ENOTDIR: 20,
          ENOTEMPTY: 66, ENOTSOCK: 38, ENOTSUP: 45, ENOTTY: 25,
          ENXIO: 6, EOPNOTSUPP: 102, EOVERFLOW: 84, EPERM: 1,
          EPIPE: 32, EPROTONOSUPPORT: 43, EPROTOTYPE: 41, ERANGE: 34,
          EROFS: 30, ESPIPE: 29, ESRCH: 3, ESTALE: 70,
          ETIMEDOUT: 60, ETXTBSY: 26, EWOULDBLOCK: 35, EXDEV: 18,
        };
        c.fs = c;
        c.os = { errno: c, signals: c };
        this._bindingConstants = c;
      }
      return this._bindingConstants;
    }
    if (name === "fs") return {};
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

// ---- IPC channel setup (Node.js child_process.fork() compatibility) ----
// When real Node forks utoo-runtime as a child, it creates a socketpair,
// passes one end as fd 3, and sets NODE_CHANNEL_FD=3. We detect this and
// wire up process.send() / process.on('message') using the same
// newline-delimited JSON protocol as Node.js.
if (__bootstrapOps.op_ipc_has_channel()) {
  globalThis.process.channel = { ref: function() {}, unref: function() {} };
  globalThis.process.connected = true;

  globalThis.process.send = function(message, handle, options, callback) {
    if (typeof handle === "function") {
      callback = handle; handle = undefined; options = undefined;
    } else if (typeof options === "function") {
      callback = options; options = undefined;
    }
    try {
      __bootstrapOps.op_ipc_send(JSON.stringify(message));
      if (typeof callback === "function") callback(null);
      return true;
    } catch (e) {
      if (typeof callback === "function") callback(e);
      return false;
    }
  };

  globalThis.process.disconnect = function() {
    globalThis.process.connected = false;
    globalThis.process.channel = null;
    globalThis.process.emit("disconnect");
  };

  // Background read loop for incoming IPC messages from parent.
  // In this deno_core version, async ops are called via Deno.core.ops directly.
  (async function __ipcReadLoop() {
    while (globalThis.process.connected) {
      try {
        var line = await __bootstrapOps.op_ipc_read_line();
        if (!line) {
          // Empty string = EOF, parent closed channel
          globalThis.process.connected = false;
          globalThis.process.channel = null;
          globalThis.process.emit("disconnect");
          break;
        }
        try {
          var message = JSON.parse(line);
          globalThis.process.emit("message", message);
        } catch (e) {
          // Invalid JSON line, skip
        }
      } catch (e) {
        globalThis.process.connected = false;
        globalThis.process.channel = null;
        globalThis.process.emit("disconnect");
        break;
      }
    }
  })();
}
