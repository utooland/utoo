function escape(str) {
  return encodeURIComponent(str);
}

function unescape(str) {
  return decodeURIComponent(str.replace(/\+/g, " "));
}

function parse(str, sep, eq, opts) {
  sep = sep || "&";
  eq = eq || "=";
  const obj = Object.create(null);
  if (typeof str !== "string" || str.length === 0) return obj;
  const pairs = str.split(sep);
  const maxKeys = (opts && opts.maxKeys) || 1000;
  const len = maxKeys > 0 ? Math.min(pairs.length, maxKeys) : pairs.length;
  for (let i = 0; i < len; i++) {
    const pair = pairs[i];
    const idx = pair.indexOf(eq);
    let key, val;
    if (idx >= 0) {
      key = unescape(pair.slice(0, idx));
      val = unescape(pair.slice(idx + eq.length));
    } else {
      key = unescape(pair);
      val = "";
    }
    if (obj[key] === undefined) {
      obj[key] = val;
    } else if (Array.isArray(obj[key])) {
      obj[key].push(val);
    } else {
      obj[key] = [obj[key], val];
    }
  }
  return obj;
}

function stringify(obj, sep, eq) {
  sep = sep || "&";
  eq = eq || "=";
  if (!obj || typeof obj !== "object") return "";
  return Object.keys(obj)
    .map((k) => {
      const v = obj[k];
      if (Array.isArray(v)) {
        return v.map((item) => escape(k) + eq + escape(String(item))).join(sep);
      }
      return escape(k) + eq + escape(String(v));
    })
    .filter(Boolean)
    .join(sep);
}

const qs = { parse, stringify, escape, unescape, decode: parse, encode: stringify };
export default qs;
export { parse, stringify, escape, unescape, parse as decode, stringify as encode };
