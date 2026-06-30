// Basic URL implementation for environments without globalThis.URL.
// Covers the most common Node.js URL API usage patterns.

class UtooURL {
  constructor(input, base) {
    let url = String(input);
    const hasScheme = /^[a-z][a-z0-9+.-]*:/i.test(url);
    if (base !== undefined && base !== null && !hasScheme) {
      const baseStr = typeof base === "string" ? base : (base.href || String(base));
      // Parse base, supporting both special (scheme://host/path) and
      // non-special / opaque (scheme:path, e.g. turbopack's `x:/`) forms.
      const bm = baseStr.match(
        /^([a-z][a-z0-9+.-]*):(\/\/([^/?#:]*)(?::(\d+))?)?([^?#]*)?(\?[^#]*)?(#.*)?$/i,
      );
      if (bm) {
        const bProto = bm[1];
        const authority = bm[2] ? "//" + bm[3] + (bm[4] ? ":" + bm[4] : "") : "";
        const bPath = bm[5] || (authority ? "/" : "");
        if (url.startsWith("/")) {
          url = bProto + ":" + authority + url;
        } else if (url.startsWith("?") || url.startsWith("#")) {
          url = bProto + ":" + authority + bPath + url;
        } else {
          const dir = bPath.replace(/\/[^/]*$/, "/") || "/";
          url = bProto + ":" + authority + dir + url;
        }
      }
    }
    // Special URLs (with `//` authority): scheme://host:port/path?query#hash
    const match = url.match(
      /^([a-z][a-z0-9+.-]*):\/\/([^/?#:]*)(?::(\d+))?(\/[^?#]*)?(\?[^#]*)?(#.*)?$/i,
    );
    if (match) {
      this.protocol = match[1] + ":";
      this.hostname = match[2] || "";
      this.port = match[3] || "";
      this.pathname = match[4] || "/";
      this.search = match[5] || "";
      this.hash = match[6] || "";
      this.host = this.port ? `${this.hostname}:${this.port}` : this.hostname;
      this.origin = `${this.protocol}//${this.host}`;
      this.href = this.origin + this.pathname + this.search + this.hash;
    } else {
      // Non-special / opaque scheme: scheme:path?query#hash (no authority).
      const m2 = url.match(/^([a-z][a-z0-9+.-]*):([^?#]*)(\?[^#]*)?(#.*)?$/i);
      if (!m2) {
        throw new TypeError(`Invalid URL: ${input}`);
      }
      this.protocol = m2[1] + ":";
      this.hostname = "";
      this.port = "";
      this.pathname = m2[2] || "";
      this.search = m2[3] || "";
      this.hash = m2[4] || "";
      this.host = "";
      this.origin = "null";
      this.href = this.protocol + this.pathname + this.search + this.hash;
    }
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

export function pathToFileURL(filepath) {
  let resolved = String(filepath);
  // Ensure absolute path
  if (!resolved.startsWith("/")) {
    resolved = Deno.core.ops.op_cwd() + "/" + resolved;
  }
  return new URL("file://" + encodeURI(resolved).replace(/#/g, "%23").replace(/\?/g, "%3F"));
}

export function fileURLToPath(url) {
  let href;
  if (typeof url === "string") {
    href = url;
  } else if (url && typeof url.href === "string") {
    href = url.href;
  } else {
    throw new TypeError("The \"path\" argument must be of type string or an instance of URL");
  }
  if (!href.startsWith("file://")) {
    throw new TypeError("The URL must be of scheme file");
  }
  return decodeURIComponent(href.slice(7));
}

export function format(urlObject, options) {
  if (typeof urlObject === "string") return urlObject;
  if (urlObject instanceof URL || (urlObject && urlObject.href)) return urlObject.href;
  // Legacy url.format
  let result = "";
  if (urlObject.protocol) result += urlObject.protocol;
  if (urlObject.slashes || urlObject.protocol === "http:" || urlObject.protocol === "https:") result += "//";
  if (urlObject.auth) result += urlObject.auth + "@";
  if (urlObject.hostname) result += urlObject.hostname;
  if (urlObject.port) result += ":" + urlObject.port;
  if (urlObject.pathname) result += urlObject.pathname;
  if (urlObject.search) result += urlObject.search;
  if (urlObject.hash) result += urlObject.hash;
  return result;
}

export function parse(urlString, parseQueryString) {
  // Handle relative URLs (starting with /) by adding a dummy origin
  let fullUrl = urlString;
  let isRelative = false;
  if (typeof urlString === "string" && urlString.startsWith("/")) {
    fullUrl = "http://localhost" + urlString;
    isRelative = true;
  }
  try {
    const u = new URL(fullUrl);
    const rawQuery = u.search ? u.search.slice(1) : null;
    let query;
    if (parseQueryString) {
      query = {};
      if (rawQuery) {
        for (const pair of rawQuery.split("&")) {
          const eq = pair.indexOf("=");
          const key = eq >= 0 ? decodeURIComponent(pair.slice(0, eq)) : decodeURIComponent(pair);
          const val = eq >= 0 ? decodeURIComponent(pair.slice(eq + 1)) : "";
          if (key in query) {
            const existing = query[key];
            query[key] = Array.isArray(existing) ? [...existing, val] : [existing, val];
          } else {
            query[key] = val;
          }
        }
      }
    } else {
      query = rawQuery;
    }
    return {
      protocol: isRelative ? null : u.protocol,
      slashes: !isRelative,
      auth: null,
      host: isRelative ? null : u.host,
      port: u.port || null,
      hostname: isRelative ? null : u.hostname,
      hash: u.hash || null,
      search: u.search || null,
      query: query,
      pathname: u.pathname,
      path: u.pathname + (u.search || ""),
      href: isRelative ? urlString : u.href,
    };
  } catch {
    const query = parseQueryString ? {} : null;
    return { protocol: null, slashes: null, auth: null, host: null, port: null, hostname: null, hash: null, search: null, query: query, pathname: urlString, path: urlString, href: urlString };
  }
}

export function resolve(from, to) {
  return new URL(to, from).href;
}

export default { URL, URLSearchParams, pathToFileURL, fileURLToPath, format, parse, resolve };
