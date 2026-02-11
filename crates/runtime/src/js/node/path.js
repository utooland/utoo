const _platform = Deno.core.ops.op_os_platform();
export const sep = _platform === "win32" ? "\\" : "/";
export const delimiter = _platform === "win32" ? ";" : ":";

export function isAbsolute(p) {
  if (_platform === "win32") {
    return /^[a-zA-Z]:[\\\/]/.test(p) || p.startsWith("\\\\");
  }
  return p.startsWith("/");
}

export function normalize(p) {
  if (p.length === 0) return ".";
  const isAbs = isAbsolute(p);
  const parts = p.split(/[\/\\]+/).filter(Boolean);
  const stack = [];
  for (const part of parts) {
    if (part === "..") {
      if (stack.length > 0 && stack[stack.length - 1] !== "..") {
        stack.pop();
      } else if (!isAbs) {
        stack.push(part);
      }
    } else if (part !== ".") {
      stack.push(part);
    }
  }
  let result = stack.join(sep);
  if (isAbs) {
    result = sep + result;
  }
  return result || ".";
}

export function join(...segments) {
  if (segments.length === 0) return ".";
  const joined = segments.filter((s) => typeof s === "string" && s.length > 0).join(sep);
  return normalize(joined);
}

export function resolve(...segments) {
  let resolved = "";
  for (let i = segments.length - 1; i >= 0; i--) {
    const seg = segments[i];
    if (typeof seg !== "string" || seg.length === 0) continue;
    resolved = resolved ? seg + sep + resolved : seg;
    if (isAbsolute(resolved)) break;
  }
  return normalize(resolved);
}

export function dirname(p) {
  if (p.length === 0) return ".";
  const i = p.lastIndexOf(sep === "\\" ? /[\/\\]/ : "/");
  const idx =
    sep === "\\"
      ? Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"))
      : p.lastIndexOf("/");
  if (idx === -1) return ".";
  if (idx === 0) return sep;
  return p.slice(0, idx);
}

export function basename(p, ext) {
  const idx =
    sep === "\\"
      ? Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"))
      : p.lastIndexOf("/");
  let base = idx === -1 ? p : p.slice(idx + 1);
  if (ext && base.endsWith(ext)) {
    base = base.slice(0, -ext.length);
  }
  return base;
}

export function extname(p) {
  const base = basename(p);
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return "";
  return base.slice(dot);
}

export function parse(p) {
  const dir = dirname(p);
  const base = basename(p);
  const ext = extname(p);
  const name = ext ? base.slice(0, -ext.length) : base;
  const root = isAbsolute(p) ? sep : "";
  return { root, dir, base, ext, name };
}

export function format(obj) {
  const dir = obj.dir || obj.root || "";
  const base = obj.base || (obj.name || "") + (obj.ext || "");
  if (!dir) return base;
  return dir === obj.root ? dir + base : dir + sep + base;
}

export function relative(from, to) {
  const fromParts = normalize(from).split(sep).filter(Boolean);
  const toParts = normalize(to).split(sep).filter(Boolean);
  let i = 0;
  while (
    i < fromParts.length &&
    i < toParts.length &&
    fromParts[i] === toParts[i]
  ) {
    i++;
  }
  const ups = fromParts.slice(i).map(() => "..");
  return [...ups, ...toParts.slice(i)].join(sep) || ".";
}

export const posix = {
  sep: "/",
  delimiter: ":",
  join,
  resolve,
  dirname,
  basename,
  extname,
  normalize,
  isAbsolute,
  relative,
  parse,
  format,
};

export const win32 = {
  sep: "\\",
  delimiter: ";",
};

export default {
  sep,
  delimiter,
  join,
  resolve,
  dirname,
  basename,
  extname,
  normalize,
  isAbsolute,
  relative,
  parse,
  format,
  posix,
  win32,
};
