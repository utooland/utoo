import { Transform } from "ext:utoo_rt_ext/node/stream";

class Gzip extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class Gunzip extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class Deflate extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class Inflate extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class DeflateRaw extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class InflateRaw extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class Unzip extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class BrotliCompress extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

class BrotliDecompress extends Transform {
  _transform(chunk, encoding, cb) { cb(null, chunk); }
}

function createGzip(opts) { return new Gzip(opts); }
function createGunzip(opts) { return new Gunzip(opts); }
function createDeflate(opts) { return new Deflate(opts); }
function createInflate(opts) { return new Inflate(opts); }
function createDeflateRaw(opts) { return new DeflateRaw(opts); }
function createInflateRaw(opts) { return new InflateRaw(opts); }
function createUnzip(opts) { return new Unzip(opts); }
function createBrotliCompress(opts) { return new BrotliCompress(opts); }
function createBrotliDecompress(opts) { return new BrotliDecompress(opts); }

// zlib convenience methods (callback-based)
function _zlibOneShotCb(data, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = {}; }
  // Stub: return data unchanged (no actual compression)
  const result = typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data);
  if (cb) { process.nextTick(() => cb(null, result)); return; }
}

function gzip(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function gunzip(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function deflate(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function inflate(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function deflateRaw(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function inflateRaw(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function unzip(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function brotliCompress(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }
function brotliDecompress(data, opts, cb) { _zlibOneShotCb(data, opts, cb); }

// Sync variants (stubs)
function gzipSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function gunzipSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function deflateSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function inflateSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function deflateRawSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function inflateRawSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function unzipSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function brotliCompressSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }
function brotliDecompressSync(data) { return typeof Buffer !== "undefined" ? Buffer.from(data) : new Uint8Array(data); }

// Constants
const constants = {
  Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2,
  Z_FULL_FLUSH: 3, Z_FINISH: 4, Z_BLOCK: 5, Z_TREES: 6,
  Z_OK: 0, Z_STREAM_END: 1, Z_NEED_DICT: 2,
  Z_ERRNO: -1, Z_STREAM_ERROR: -2, Z_DATA_ERROR: -3,
  Z_MEM_ERROR: -4, Z_BUF_ERROR: -5, Z_VERSION_ERROR: -6,
  Z_NO_COMPRESSION: 0, Z_BEST_SPEED: 1, Z_BEST_COMPRESSION: 9,
  Z_DEFAULT_COMPRESSION: -1,
  Z_FILTERED: 1, Z_HUFFMAN_ONLY: 2, Z_RLE: 3, Z_FIXED: 4,
  Z_DEFAULT_STRATEGY: 0,
  Z_DEFAULT_WINDOWBITS: 15, Z_MIN_WINDOWBITS: 8, Z_MAX_WINDOWBITS: 15,
  Z_MIN_CHUNK: 64, Z_MAX_CHUNK: Infinity, Z_DEFAULT_CHUNK: 16384,
  Z_MIN_MEMLEVEL: 1, Z_MAX_MEMLEVEL: 9, Z_DEFAULT_MEMLEVEL: 8,
  Z_MIN_LEVEL: -1, Z_MAX_LEVEL: 9,
  BROTLI_OPERATION_PROCESS: 0, BROTLI_OPERATION_FLUSH: 1,
  BROTLI_OPERATION_FINISH: 2, BROTLI_OPERATION_EMIT_METADATA: 3,
};

const zlib = {
  Gzip, Gunzip, Deflate, Inflate, DeflateRaw, InflateRaw,
  Unzip, BrotliCompress, BrotliDecompress,
  createGzip, createGunzip, createDeflate, createInflate,
  createDeflateRaw, createInflateRaw, createUnzip,
  createBrotliCompress, createBrotliDecompress,
  gzip, gunzip, deflate, inflate, deflateRaw, inflateRaw, unzip,
  brotliCompress, brotliDecompress,
  gzipSync, gunzipSync, deflateSync, inflateSync,
  deflateRawSync, inflateRawSync, unzipSync,
  brotliCompressSync, brotliDecompressSync,
  constants,
  // Legacy constants at top level too
  Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2,
  Z_FULL_FLUSH: 3, Z_FINISH: 4,
};
export default zlib;
export {
  Gzip, Gunzip, Deflate, Inflate, DeflateRaw, InflateRaw,
  Unzip, BrotliCompress, BrotliDecompress,
  createGzip, createGunzip, createDeflate, createInflate,
  createDeflateRaw, createInflateRaw, createUnzip,
  createBrotliCompress, createBrotliDecompress,
  gzip, gunzip, deflate, inflate, deflateRaw, inflateRaw, unzip,
  brotliCompress, brotliDecompress,
  gzipSync, gunzipSync, deflateSync, inflateSync,
  deflateRawSync, inflateRawSync, unzipSync,
  brotliCompressSync, brotliDecompressSync,
  constants,
};
