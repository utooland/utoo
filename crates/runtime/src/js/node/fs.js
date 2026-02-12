import * as promises from "ext:utoo_rt_ext/node/fs_promises";
import { Buffer } from "ext:utoo_rt_ext/node/buffer";
export { promises };

const ops = Deno.core.ops;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

function normalizeArgs(optionsOrCb, cb) {
  if (typeof optionsOrCb === "function") {
    return [undefined, optionsOrCb];
  }
  return [optionsOrCb, cb];
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

function toBuffer(data) {
  return typeof data === "string" ? Buffer.from(data, "utf-8") : data;
}

// ---------------------------------------------------------------------------
// Sync APIs
// ---------------------------------------------------------------------------

export function readFileSync(path, options) {
  const encoding =
    typeof options === "string" ? options : options?.encoding;
  if (encoding === "utf8" || encoding === "utf-8") {
    return ops.op_fs_read_text_file_sync(String(path));
  }
  const data = ops.op_fs_read_file_sync(String(path));
  return Buffer.from(data.buffer || data, data.byteOffset, data.byteLength);
}

export function writeFileSync(path, data, _options) {
  ops.op_fs_write_file_sync(String(path), toBuffer(data));
}

export function appendFileSync(path, data, _options) {
  ops.op_fs_append_file_sync(String(path), toBuffer(data));
}

export function readdirSync(path, options) {
  try {
    const entries = ops.op_fs_readdir_sync(String(path));
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
  } catch (err) {
    throw wrapFsError(err, "scandir", String(path));
  }
}

export function mkdirSync(path, options) {
  const recursive =
    typeof options === "object" ? !!options?.recursive : false;
  ops.op_fs_mkdir_sync(String(path), recursive);
}

function wrapFsError(err, syscall, path) {
  const msg = err.message || String(err);
  if (msg.includes("No such file or directory") || msg.includes("os error 2")) {
    const e = new Error(`${syscall} '${path}': ENOENT: no such file or directory`);
    e.code = "ENOENT";
    e.errno = -2;
    e.syscall = syscall;
    e.path = path;
    return e;
  }
  if (msg.includes("Permission denied") || msg.includes("os error 13")) {
    const e = new Error(`${syscall} '${path}': EACCES: permission denied`);
    e.code = "EACCES";
    e.errno = -13;
    e.syscall = syscall;
    e.path = path;
    return e;
  }
  err.syscall = syscall;
  err.path = path;
  return err;
}

export function statSync(path, options) {
  try {
    return wrapStats(ops.op_fs_stat_sync(String(path)));
  } catch (err) {
    if (options && options.throwIfNoEntry === false) return undefined;
    throw wrapFsError(err, "stat", String(path));
  }
}

export function lstatSync(path, options) {
  try {
    return wrapStats(ops.op_fs_lstat_sync(String(path)));
  } catch (err) {
    if (options && options.throwIfNoEntry === false) return undefined;
    throw wrapFsError(err, "lstat", String(path));
  }
}

export function unlinkSync(path) {
  ops.op_fs_unlink_sync(String(path));
}

export function renameSync(oldPath, newPath) {
  ops.op_fs_rename_sync(String(oldPath), String(newPath));
}

export function copyFileSync(src, dest) {
  ops.op_fs_copy_file_sync(String(src), String(dest));
}

export function rmSync(path, options) {
  const recursive =
    typeof options === "object" ? !!options?.recursive : false;
  ops.op_fs_rm_sync(String(path), recursive);
}

export function existsSync(path) {
  return ops.op_fs_exists_sync(String(path));
}

export function accessSync(path, mode) {
  ops.op_fs_access_sync(String(path), mode ?? 0);
}

export function chmodSync(path, mode) {
  ops.op_fs_chmod_sync(String(path), mode);
}

export function realpathSync(path) {
  try { return ops.op_fs_realpath_sync(String(path)); } catch (e) { __fsErr(e); }
}
// Node.js provides realpathSync.native as the native implementation
realpathSync.native = realpathSync;

// ---------------------------------------------------------------------------
// Callback APIs
// ---------------------------------------------------------------------------

export function readFile(path, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.readFile(path, options).then(
    (data) => callback(null, data),
    (err) => callback(err),
  );
}

export function writeFile(path, data, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.writeFile(path, data, options).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function appendFile(path, data, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.appendFile(path, data, options).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function readdir(path, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.readdir(path, options).then(
    (entries) => callback(null, entries),
    (err) => callback(err),
  );
}

export function mkdir(path, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.mkdir(path, options).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function stat(path, optionsOrCb, cb) {
  const [_options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.stat(path).then(
    (s) => callback(null, s),
    (err) => callback(err),
  );
}

export function lstat(path, optionsOrCb, cb) {
  const [_options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.lstat(path).then(
    (s) => callback(null, s),
    (err) => callback(err),
  );
}

export function unlink(path, cb) {
  promises.unlink(path).then(
    () => cb(null),
    (err) => cb(err),
  );
}

export function rename(oldPath, newPath, cb) {
  promises.rename(oldPath, newPath).then(
    () => cb(null),
    (err) => cb(err),
  );
}

export function copyFile(src, dest, flagsOrCb, cb) {
  const callback = typeof flagsOrCb === "function" ? flagsOrCb : cb;
  promises.copyFile(src, dest).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function rm(path, optionsOrCb, cb) {
  const [options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.rm(path, options).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function access(path, modeOrCb, cb) {
  const [mode, callback] =
    typeof modeOrCb === "function" ? [undefined, modeOrCb] : [modeOrCb, cb];
  promises.access(path, mode).then(
    () => callback(null),
    (err) => callback(err),
  );
}

export function chmod(path, mode, cb) {
  promises.chmod(path, mode).then(
    () => cb(null),
    (err) => cb(err),
  );
}

export function realpath(path, optionsOrCb, cb) {
  const [_options, callback] = normalizeArgs(optionsOrCb, cb);
  promises.realpath(path).then(
    (p) => callback(null, p),
    (err) => callback(err),
  );
}

export function exists(path, cb) {
  promises
    .access(path)
    .then(
      () => cb(true),
      () => cb(false),
    );
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const constants = {
  F_OK: 0,
  R_OK: 4,
  W_OK: 2,
  X_OK: 1,
};

// ---------------------------------------------------------------------------
// Stream APIs (minimal implementations for Node.js compat)
// ---------------------------------------------------------------------------

import { Writable, Readable } from "ext:utoo_rt_ext/node/stream";

export function createWriteStream(path, options) {
  const encoding = options?.encoding || "utf8";
  const flags = options?.flags || "w";
  const isAppend = flags.includes("a");
  let opened = false;

  const stream = new Writable({
    write(chunk, enc, cb) {
      try {
        const data = typeof chunk === "string" ? chunk : Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
        if (!opened) {
          if (!isAppend) ops.op_fs_write_file_sync(String(path), toBuffer(data));
          else ops.op_fs_append_file_sync(String(path), toBuffer(data));
          opened = true;
        } else {
          ops.op_fs_append_file_sync(String(path), toBuffer(data));
        }
        cb();
      } catch (err) {
        cb(err);
      }
    },
  });

  stream.path = path;
  // emit 'open' and 'close' asynchronously
  queueMicrotask(() => stream.emit("open"));
  return stream;
}

export function createReadStream(path, options) {
  const encoding = options?.encoding || null;
  const stream = new Readable({
    read() {
      try {
        const data = readFileSync(path, encoding ? { encoding } : undefined);
        this.push(data);
        this.push(null);
      } catch (err) {
        this.destroy(err);
      }
    },
  });

  stream.path = path;
  queueMicrotask(() => stream.emit("open"));
  return stream;
}

export function watch(filename, options, listener) {
  // Stub - just return an event emitter
  if (typeof options === "function") { listener = options; options = {}; }
  const watcher = new (require("events"))();
  watcher.close = function() {};
  return watcher;
}

export function watchFile() {}
export function unwatchFile() {}

// ---------------------------------------------------------------------------
// Default export
// ---------------------------------------------------------------------------

const fs = {
  promises,
  constants,
  // Sync
  readFileSync,
  writeFileSync,
  appendFileSync,
  readdirSync,
  mkdirSync,
  statSync,
  lstatSync,
  unlinkSync,
  renameSync,
  copyFileSync,
  rmSync,
  existsSync,
  accessSync,
  chmodSync,
  realpathSync,
  // Callback
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
  exists,
  // Streams
  createWriteStream,
  createReadStream,
  // Watch (stubs)
  watch,
  watchFile,
  unwatchFile,
};

export default fs;
