import { Buffer } from "ext:utoo_rt_ext/node/buffer";

const ops = Deno.core.ops;

// Add Node.js-compatible .code property to fs errors (e.g., "ENOENT")
function __fsErr(e) {
  if (e && !e.code && typeof e.message === "string") {
    const colon = e.message.indexOf(":");
    if (colon > 0 && colon < 12) {
      const code = e.message.slice(0, colon);
      if (/^[A-Z]+$/.test(code)) e.code = code;
    }
  }
  throw e;
}

function toBytes(data) {
  if (typeof data === "string") return Buffer.from(data, "utf-8");
  return data;
}

function wrapStats(s) {
  return {
    size: s.size,
    isFile: () => s.is_file,
    isDirectory: () => s.is_directory,
    isSymbolicLink: () => s.is_symlink,
    mode: s.mode,
    mtimeMs: s.mtime_ms,
    atimeMs: s.atime_ms,
    ctimeMs: s.ctime_ms,
    birthtimeMs: s.birthtime_ms,
    mtime: new Date(s.mtime_ms),
    atime: new Date(s.atime_ms),
    ctime: new Date(s.ctime_ms),
    birthtime: new Date(s.birthtime_ms),
    dev: s.dev,
    ino: s.ino,
    nlink: s.nlink,
  };
}

export async function readFile(path, options) {
  try {
    const encoding =
      typeof options === "string" ? options : options?.encoding;
    if (encoding === "utf8" || encoding === "utf-8") {
      return await ops.op_fs_read_text_file(String(path));
    }
    const buf = await ops.op_fs_read_file(String(path));
    return Buffer.from(buf);
  } catch (e) { __fsErr(e); }
}

export async function writeFile(path, data, _options) {
  await ops.op_fs_write_file(String(path), toBytes(data));
}

export async function appendFile(path, data, _options) {
  await ops.op_fs_append_file(String(path), toBytes(data));
}

export async function readdir(path, options) {
  const entries = await ops.op_fs_readdir(String(path));
  if (options && options.withFileTypes) {
    return entries.map((e) => ({
      name: e.name,
      isFile: () => e.is_file,
      isDirectory: () => e.is_directory,
      isSymbolicLink: () => e.is_symlink || false,
      isBlockDevice: () => false,
      isCharacterDevice: () => false,
      isFIFO: () => false,
      isSocket: () => false,
    }));
  }
  return entries.map((e) => e.name);
}

export async function mkdir(path, options) {
  const recursive =
    typeof options === "object" ? !!options?.recursive : false;
  await ops.op_fs_mkdir(String(path), recursive);
}

export async function stat(path) {
  try { return wrapStats(await ops.op_fs_stat(String(path))); } catch (e) { __fsErr(e); }
}

export async function lstat(path) {
  try { return wrapStats(await ops.op_fs_lstat(String(path))); } catch (e) { __fsErr(e); }
}

export async function unlink(path) {
  await ops.op_fs_unlink(String(path));
}

export async function rename(oldPath, newPath) {
  await ops.op_fs_rename(String(oldPath), String(newPath));
}

export async function copyFile(src, dest) {
  await ops.op_fs_copy_file(String(src), String(dest));
}

export async function rm(path, options) {
  const recursive =
    typeof options === "object" ? !!options?.recursive : false;
  await ops.op_fs_rm(String(path), recursive);
}

export async function access(path, mode) {
  try { await ops.op_fs_access(String(path), mode ?? 0); } catch (e) { __fsErr(e); }
}

export async function chmod(path, mode) {
  await ops.op_fs_chmod(String(path), mode);
}

export async function realpath(path) {
  return await ops.op_fs_realpath(String(path));
}

export default {
  readFile,
  writeFile,
  appendFile,
  readdir,
  mkdir,
  stat,
  lstat,
  unlink,
  rename,
  copyFile,
  rm,
  access,
  chmod,
  realpath,
};
