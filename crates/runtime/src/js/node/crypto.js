const ops = Deno.core.ops;
const encoder = new TextEncoder();

function toBytes(data, inputEncoding) {
  if (data instanceof Uint8Array) return data;
  if (typeof data === "string") {
    if (inputEncoding === "hex") {
      const bytes = new Uint8Array(data.length / 2);
      for (let i = 0; i < data.length; i += 2) {
        bytes[i / 2] = parseInt(data.substr(i, 2), 16);
      }
      return bytes;
    }
    if (inputEncoding === "base64") {
      const bin = atob(data);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      return bytes;
    }
    return encoder.encode(data);
  }
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new Uint8Array(data);
}

function toOutput(bytes, encoding) {
  if (!encoding || encoding === "buffer") {
    // Return a Buffer if available, otherwise Uint8Array
    if (typeof Buffer !== "undefined") return Buffer.from(bytes);
    return new Uint8Array(bytes);
  }
  if (encoding === "hex") {
    return Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
  }
  if (encoding === "base64") {
    let bin = "";
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
    return btoa(bin);
  }
  // latin1/binary
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
  return s;
}

// Pure-JS MD5 (ring doesn't support MD5)
function __md5(data) {
  var d = data;
  var n = d.length;
  var s = [7,12,17,22,7,12,17,22,7,12,17,22,7,12,17,22,
           5,9,14,20,5,9,14,20,5,9,14,20,5,9,14,20,
           4,11,16,23,4,11,16,23,4,11,16,23,4,11,16,23,
           6,10,15,21,6,10,15,21,6,10,15,21,6,10,15,21];
  var K = new Uint32Array(64);
  for (var i = 0; i < 64; i++) K[i] = Math.floor(4294967296 * Math.abs(Math.sin(i + 1)));
  // Pad
  var bitLen = n * 8;
  var padLen = ((56 - (n + 1) % 64) + 64) % 64;
  var buf = new Uint8Array(n + 1 + padLen + 8);
  buf.set(d);
  buf[n] = 0x80;
  for (var i = 0; i < 4; i++) buf[buf.length - 8 + i] = (bitLen >>> (i * 8)) & 0xff;
  var a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
  var view = new DataView(buf.buffer);
  for (var off = 0; off < buf.length; off += 64) {
    var M = new Uint32Array(16);
    for (var j = 0; j < 16; j++) M[j] = view.getUint32(off + j * 4, true);
    var A = a0, B = b0, C = c0, D = d0;
    for (var i = 0; i < 64; i++) {
      var F, g;
      if (i < 16) { F = (B & C) | (~B & D); g = i; }
      else if (i < 32) { F = (D & B) | (~D & C); g = (5 * i + 1) % 16; }
      else if (i < 48) { F = B ^ C ^ D; g = (3 * i + 5) % 16; }
      else { F = C ^ (B | ~D); g = (7 * i) % 16; }
      F = (F + A + K[i] + M[g]) >>> 0;
      A = D; D = C; C = B;
      B = (B + ((F << s[i]) | (F >>> (32 - s[i])))) >>> 0;
    }
    a0 = (a0 + A) >>> 0; b0 = (b0 + B) >>> 0; c0 = (c0 + C) >>> 0; d0 = (d0 + D) >>> 0;
  }
  var r = new Uint8Array(16);
  for (var i = 0; i < 4; i++) { r[i] = (a0 >>> (i*8)) & 0xff; r[i+4] = (b0 >>> (i*8)) & 0xff; r[i+8] = (c0 >>> (i*8)) & 0xff; r[i+12] = (d0 >>> (i*8)) & 0xff; }
  return r;
}

function createHash(algorithm) {
  var algoLower = algorithm.toLowerCase().replace('-', '');
  if (algoLower === "md5") {
    var chunks = [];
    return {
      update(data, inputEncoding) { chunks.push(toBytes(data, inputEncoding)); return this; },
      digest(encoding) {
        var total = 0; for (var c of chunks) total += c.length;
        var buf = new Uint8Array(total); var off = 0;
        for (var c of chunks) { buf.set(c, off); off += c.length; }
        return toOutput(__md5(buf), encoding);
      },
    };
  }
  const rid = ops.op_crypto_hash_create(algorithm);
  return {
    update(data, inputEncoding) {
      ops.op_crypto_hash_update(rid, toBytes(data, inputEncoding));
      return this;
    },
    digest(encoding) {
      const bytes = ops.op_crypto_hash_digest(rid);
      return toOutput(new Uint8Array(bytes), encoding);
    },
  };
}

function createHmac(algorithm, key) {
  const keyBytes = toBytes(key);
  const rid = ops.op_crypto_hmac_create(algorithm, keyBytes);
  return {
    update(data, inputEncoding) {
      ops.op_crypto_hmac_update(rid, toBytes(data, inputEncoding));
      return this;
    },
    digest(encoding) {
      const bytes = ops.op_crypto_hmac_digest(rid);
      return toOutput(new Uint8Array(bytes), encoding);
    },
  };
}

function randomBytes(size, cb) {
  const bytes = ops.op_crypto_random_bytes(size);
  const buf = typeof Buffer !== "undefined" ? Buffer.from(bytes) : new Uint8Array(bytes);
  if (cb) {
    process.nextTick(() => cb(null, buf));
    return;
  }
  return buf;
}

function timingSafeEqual(a, b) {
  if (a.length !== b.length) throw new RangeError("Input buffers must have the same byte length");
  let result = 0;
  for (let i = 0; i < a.length; i++) result |= a[i] ^ b[i];
  return result === 0;
}

function randomUUID() {
  const bytes = new Uint8Array(ops.op_crypto_random_bytes(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
  return hex.slice(0, 8) + "-" + hex.slice(8, 12) + "-" + hex.slice(12, 16) + "-" + hex.slice(16, 20) + "-" + hex.slice(20);
}

function randomInt(min, max, cb) {
  if (typeof max === "function") { cb = max; max = min; min = 0; }
  const range = max - min;
  const bytes = new Uint8Array(ops.op_crypto_random_bytes(4));
  const val = min + ((bytes[0] | (bytes[1] << 8) | (bytes[2] << 16) | ((bytes[3] & 0x7f) << 24)) % range);
  if (cb) { process.nextTick(() => cb(null, val)); return; }
  return val;
}

function randomFillSync(buf, offset, size) {
  offset = offset || 0;
  size = size !== undefined ? size : buf.length - offset;
  const bytes = new Uint8Array(ops.op_crypto_random_bytes(size));
  for (let i = 0; i < size; i++) buf[offset + i] = bytes[i];
  return buf;
}

function randomFill(buf, offset, size, cb) {
  if (typeof offset === "function") { cb = offset; offset = 0; size = buf.length; }
  else if (typeof size === "function") { cb = size; size = buf.length - offset; }
  try {
    randomFillSync(buf, offset, size);
    if (cb) process.nextTick(() => cb(null, buf));
  } catch (e) {
    if (cb) process.nextTick(() => cb(e));
    else throw e;
  }
  return buf;
}

// Web Crypto API shim for Node.js crypto.webcrypto
const webcrypto = {
  getRandomValues(arr) { return globalThis.crypto.getRandomValues(arr); },
  randomUUID() { return randomUUID(); },
  subtle: globalThis.crypto.subtle,
  CryptoKey: globalThis.CryptoKey,
};

function getCiphers() {
  return ["aes-128-cbc","aes-128-gcm","aes-192-cbc","aes-192-gcm","aes-256-cbc","aes-256-gcm","des-ede3-cbc","rc4"];
}

function getHashes() {
  return ["md5","sha1","sha256","sha384","sha512"];
}

function getCurves() {
  return ["prime256v1","secp384r1","secp521r1"];
}

function createCipheriv(algorithm, key, iv, options) {
  throw new Error("crypto.createCipheriv is not yet implemented in utoo-runtime");
}

function createDecipheriv(algorithm, key, iv, options) {
  throw new Error("crypto.createDecipheriv is not yet implemented in utoo-runtime");
}

function createSign(algorithm) {
  throw new Error("crypto.createSign is not yet implemented in utoo-runtime");
}

function createVerify(algorithm) {
  throw new Error("crypto.createVerify is not yet implemented in utoo-runtime");
}

function publicEncrypt(key, buffer) {
  throw new Error("crypto.publicEncrypt is not yet implemented in utoo-runtime");
}

function privateDecrypt(key, buffer) {
  throw new Error("crypto.privateDecrypt is not yet implemented in utoo-runtime");
}

function privateEncrypt(key, buffer) {
  throw new Error("crypto.privateEncrypt is not yet implemented in utoo-runtime");
}

function publicDecrypt(key, buffer) {
  throw new Error("crypto.publicDecrypt is not yet implemented in utoo-runtime");
}

function pbkdf2(password, salt, iterations, keylen, digest, cb) {
  throw new Error("crypto.pbkdf2 is not yet implemented in utoo-runtime");
}

function pbkdf2Sync(password, salt, iterations, keylen, digest) {
  throw new Error("crypto.pbkdf2Sync is not yet implemented in utoo-runtime");
}

function scrypt(password, salt, keylen, options, cb) {
  throw new Error("crypto.scrypt is not yet implemented in utoo-runtime");
}

function scryptSync(password, salt, keylen, options) {
  throw new Error("crypto.scryptSync is not yet implemented in utoo-runtime");
}

function createDiffieHellman() {
  throw new Error("crypto.createDiffieHellman is not yet implemented in utoo-runtime");
}

function createECDH(curveName) {
  throw new Error("crypto.createECDH is not yet implemented in utoo-runtime");
}

function generateKeyPairSync(type, options) {
  throw new Error("crypto.generateKeyPairSync is not yet implemented in utoo-runtime");
}

function generateKeyPair(type, options, cb) {
  throw new Error("crypto.generateKeyPair is not yet implemented in utoo-runtime");
}

function sign(algorithm, data, key, cb) {
  throw new Error("crypto.sign is not yet implemented in utoo-runtime");
}

function verify(algorithm, data, key, signature, cb) {
  throw new Error("crypto.verify is not yet implemented in utoo-runtime");
}

const cryptoConstants = {
  SSL_OP_ALL: 0,
  ENGINE_METHOD_RSA: 0x0001,
  DH_CHECK_P_NOT_SAFE_PRIME: 0x02,
  POINT_CONVERSION_COMPRESSED: 2,
  POINT_CONVERSION_UNCOMPRESSED: 4,
};

const crypto = {
  createHash, createHmac, randomBytes, randomFillSync, randomFill,
  timingSafeEqual, randomUUID, randomInt,
  getCiphers, getHashes, getCurves,
  createCipheriv, createDecipheriv,
  createSign, createVerify,
  publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
  pbkdf2, pbkdf2Sync, scrypt, scryptSync,
  createDiffieHellman, createECDH,
  generateKeyPairSync, generateKeyPair,
  sign, verify,
  webcrypto,
  constants: cryptoConstants,
};
export default crypto;
export {
  createHash, createHmac, randomBytes, randomFillSync, randomFill,
  timingSafeEqual, randomUUID, randomInt, webcrypto,
  getCiphers, getHashes, getCurves,
  createCipheriv, createDecipheriv,
  createSign, createVerify,
  publicEncrypt, privateDecrypt, privateEncrypt, publicDecrypt,
  pbkdf2, pbkdf2Sync, scrypt, scryptSync,
  createDiffieHellman, createECDH,
  generateKeyPairSync, generateKeyPair,
  sign, verify,
  cryptoConstants as constants,
};
