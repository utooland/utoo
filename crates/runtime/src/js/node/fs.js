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

// node:fs Stats class. Some libraries (e.g. @eggjs/core's ManifestLoaderFS)
// build virtual stat objects via `Object.create(fs.Stats.prototype)`, so the
// class and its prototype must exist.
class Stats {
  isFile() { return (this.mode & 0o170000) === 0o100000; }
  isDirectory() { return (this.mode & 0o170000) === 0o040000; }
  isSymbolicLink() { return (this.mode & 0o170000) === 0o120000; }
  isBlockDevice() { return false; }
  isCharacterDevice() { return false; }
  isFIFO() { return false; }
  isSocket() { return false; }
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
      var dir = String(path);
      return entries.map((e) => ({
        name: e.name,
        parentPath: dir,
        path: dir,
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
  // Access modes
  F_OK: 0,
  R_OK: 4,
  W_OK: 2,
  X_OK: 1,
  // File open flags (macOS/POSIX)
  O_RDONLY: 0,
  O_WRONLY: 1,
  O_RDWR: 2,
  O_CREAT: 512,
  O_EXCL: 2048,
  O_NOCTTY: 131072,
  O_TRUNC: 1024,
  O_APPEND: 8,
  O_DIRECTORY: 1048576,
  O_NOFOLLOW: 256,
  O_SYNC: 128,
  O_DSYNC: 4194304,
  O_NONBLOCK: 4,
  // File type flags
  S_IFMT: 61440,
  S_IFREG: 32768,
  S_IFDIR: 16384,
  S_IFCHR: 8192,
  S_IFBLK: 24576,
  S_IFIFO: 4096,
  S_IFLNK: 40960,
  S_IFSOCK: 49152,
  // Permission bits
  S_IRWXU: 448,
  S_IRUSR: 256,
  S_IWUSR: 128,
  S_IXUSR: 64,
  S_IRWXG: 56,
  S_IRGRP: 32,
  S_IWGRP: 16,
  S_IXGRP: 8,
  S_IRWXO: 7,
  S_IROTH: 4,
  S_IWOTH: 2,
  S_IXOTH: 1,
  // Copy file flags
  COPYFILE_EXCL: 1,
  COPYFILE_FICLONE: 2,
  COPYFILE_FICLONE_FORCE: 4,
};

// ---------------------------------------------------------------------------
// Stream APIs (minimal implementations for Node.js compat)
// ---------------------------------------------------------------------------

import { Writable, Readable } from "ext:utoo_rt_ext/node/stream";

// ReadStream/WriteStream constructors for instanceof checks (used by 'destroy' module)
function ReadStream(path, options) {
  if (!(this instanceof ReadStream)) return new ReadStream(path, options);
  var encoding = options?.encoding || null;
  Readable.call(this, {
    read() {
      try {
        var data = readFileSync(path, encoding ? { encoding } : undefined);
        this.push(data);
        this.push(null);
      } catch (err) {
        this.destroy(err);
      }
    },
  });
  this.path = path;
  var self = this;
  queueMicrotask(function () { self.emit("open"); });
}
Object.setPrototypeOf(ReadStream.prototype, Readable.prototype);
ReadStream.prototype.constructor = ReadStream;
ReadStream.prototype.close = function (cb) { if (cb) cb(); this.destroy(); };

function WriteStream(path, options) {
  if (!(this instanceof WriteStream)) return new WriteStream(path, options);
  var flags = options?.flags || "w";
  var isAppend = flags.includes("a");
  var opened = false;
  Writable.call(this, {
    write(chunk, enc, cb) {
      try {
        var data = typeof chunk === "string" ? chunk : Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
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
  this.path = path;
  var self = this;
  queueMicrotask(function () { self.emit("open"); });
}
Object.setPrototypeOf(WriteStream.prototype, Writable.prototype);
WriteStream.prototype.constructor = WriteStream;
WriteStream.prototype.close = function (cb) { if (cb) cb(); this.destroy(); };

export function createWriteStream(path, options) {
  return new WriteStream(path, options);
}

export function createReadStream(path, options) {
  return new ReadStream(path, options);
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

// Stubs for less-common fs functions (noop or throw)
export function utimes(path, atime, mtime, cb) {
  // No-op, call callback immediately
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function utimesSync() {}
export function chown(path, uid, gid, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function chownSync() {}
export function lchown(path, uid, gid, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function lchownSync() {}
export function fstat(fd, optionsOrCb, cb) {
  var callback = typeof optionsOrCb === "function" ? optionsOrCb : cb;
  if (typeof callback === "function") queueMicrotask(() => callback(new Error("fstat not implemented")));
}
export function fstatSync() { throw new Error("fstat not implemented"); }
export function closeSync() {}
export function close(fd, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function openSync() { return 0; }
export function open(path, flags, modeOrCb, cb) {
  var callback = typeof modeOrCb === "function" ? modeOrCb : cb;
  if (typeof callback === "function") queueMicrotask(() => callback(null, 0));
}
export function fdatasyncSync() {}
export function fdatasync(fd, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function fsyncSync() {}
export function fsync(fd, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function ftruncateSync() {}
export function ftruncate(fd, lenOrCb, cb) {
  var callback = typeof lenOrCb === "function" ? lenOrCb : cb;
  if (typeof callback === "function") queueMicrotask(() => callback(null));
}
export function futimesSync() {}
export function futimes(fd, atime, mtime, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function linkSync(existingPath, newPath) {}
export function link(existingPath, newPath, cb) {
  if (typeof cb === "function") queueMicrotask(() => cb(null));
}
export function symlinkSync(target, path) {}
export function symlink(target, path, typeOrCb, cb) {
  var callback = typeof typeOrCb === "function" ? typeOrCb : cb;
  if (typeof callback === "function") queueMicrotask(() => callback(null));
}
export function readlinkSync(path) { return realpathSync(path); }
export function readlink(path, optionsOrCb, cb) {
  var callback = typeof optionsOrCb === "function" ? optionsOrCb : cb;
  realpath(path, callback);
}
export function truncateSync(path, len) {}
export function truncate(path, lenOrCb, cb) {
  var callback = typeof lenOrCb === "function" ? lenOrCb : cb;
  if (typeof callback === "function") queueMicrotask(() => callback(null));
}
export function mkdtempSync(prefix) {
  var p = prefix + Math.random().toString(36).slice(2, 8);
  mkdirSync(p, { recursive: true });
  return p;
}
export function mkdtemp(prefix, optionsOrCb, cb) {
  var callback = typeof optionsOrCb === "function" ? optionsOrCb : cb;
  if (typeof callback === "function") {
    try { var r = mkdtempSync(prefix); queueMicrotask(() => callback(null, r)); }
    catch (e) { queueMicrotask(() => callback(e)); }
  }
}
export function read() { throw new Error("fs.read not implemented"); }
export function readSync() { throw new Error("fs.readSync not implemented"); }
export function write() { throw new Error("fs.write not implemented"); }
export function writeSync() { throw new Error("fs.writeSync not implemented"); }
export function rmdirSync(path, options) { return rmSync(path, { ...options, recursive: true }); }
export function rmdir(path, optionsOrCb, cb) {
  var callback = typeof optionsOrCb === "function" ? optionsOrCb : cb;
  var opts = typeof optionsOrCb === "object" ? optionsOrCb : {};
  rm(path, { ...opts, recursive: true }, callback);
}

// ---------------------------------------------------------------------------
// Default export
// ---------------------------------------------------------------------------

const fs = {
  promises,
  constants,
  Stats,
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
  ReadStream,
  WriteStream,
  // Watch (stubs)
  watch,
  watchFile,
  unwatchFile,
  // Additional fs functions
  utimes, utimesSync,
  chown, chownSync,
  lchown, lchownSync,
  fstat, fstatSync,
  close, closeSync,
  open, openSync,
  fdatasync, fdatasyncSync,
  fsync, fsyncSync,
  ftruncate, ftruncateSync,
  futimes, futimesSync,
  link, linkSync,
  symlink, symlinkSync,
  readlink, readlinkSync,
  truncate, truncateSync,
  mkdtemp, mkdtempSync,
  read, readSync,
  write, writeSync,
  rmdirSync, rmdir,
};

export { Stats };
export default fs;
