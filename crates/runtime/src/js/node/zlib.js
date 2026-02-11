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

function createGzip() { return new Gzip(); }
function createGunzip() { return new Gunzip(); }
function createDeflate() { return new Deflate(); }
function createInflate() { return new Inflate(); }
function createDeflateRaw() { return new DeflateRaw(); }
function createInflateRaw() { return new InflateRaw(); }

const zlib = {
  Gzip, Gunzip, Deflate, Inflate, DeflateRaw, InflateRaw,
  createGzip, createGunzip, createDeflate, createInflate,
  createDeflateRaw, createInflateRaw,
  Z_NO_FLUSH: 0, Z_PARTIAL_FLUSH: 1, Z_SYNC_FLUSH: 2,
  Z_FULL_FLUSH: 3, Z_FINISH: 4,
};
export default zlib;
export {
  Gzip, Gunzip, Deflate, Inflate,
  createGzip, createGunzip, createDeflate, createInflate,
};
