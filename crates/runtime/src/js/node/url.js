// Basic URL implementation for environments without globalThis.URL.
// Covers the most common Node.js URL API usage patterns.

class UtooURL {
  constructor(input, base) {
    let url = input;
    if (base) {
      // Resolve relative URL against base
      if (!url.includes("://")) {
        url = base.replace(/\/[^/]*$/, "/") + url;
      }
    }
    const match = url.match(
      /^([a-z][a-z0-9+.-]*):\/\/([^/?#:]*)(?::(\d+))?([^?#]*)(\?[^#]*)?(#.*)?$/i,
    );
    if (!match) {
      throw new TypeError(`Invalid URL: ${input}`);
    }
    this.protocol = match[1] + ":";
    this.hostname = match[2] || "";
    this.port = match[3] || "";
    this.pathname = match[4] || "/";
    this.search = match[5] || "";
    this.hash = match[6] || "";
    this.host = this.port ? `${this.hostname}:${this.port}` : this.hostname;
    this.origin = `${this.protocol}//${this.host}`;
    this.href = url;
    this.searchParams = new UtooURLSearchParams(this.search.slice(1));
  }

  toString() {
    return this.href;
  }

  toJSON() {
    return this.href;
  }
}

class UtooURLSearchParams {
  #entries = [];

  constructor(init) {
    if (typeof init === "string") {
      if (init.startsWith("?")) init = init.slice(1);
      if (init.length > 0) {
        for (const pair of init.split("&")) {
          const [key, ...rest] = pair.split("=");
          this.#entries.push([
            decodeURIComponent(key),
            decodeURIComponent(rest.join("=")),
          ]);
        }
      }
    }
  }

  get(name) {
    for (const [k, v] of this.#entries) {
      if (k === name) return v;
    }
    return null;
  }

  has(name) {
    return this.#entries.some(([k]) => k === name);
  }

  toString() {
    return this.#entries
      .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
      .join("&");
  }

  forEach(cb) {
    for (const [k, v] of this.#entries) cb(v, k, this);
  }

  *entries() {
    yield* this.#entries;
  }

  *keys() {
    for (const [k] of this.#entries) yield k;
  }

  *values() {
    for (const [, v] of this.#entries) yield v;
  }

  [Symbol.iterator]() {
    return this.entries();
  }
}

// Use native implementations if available, otherwise use our polyfill
export const URL = globalThis.URL || UtooURL;
export const URLSearchParams = globalThis.URLSearchParams || UtooURLSearchParams;

export default { URL, URLSearchParams };
