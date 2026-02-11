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

function createHash(algorithm) {
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

const crypto = {
  createHash, createHmac, randomBytes, timingSafeEqual,
  randomUUID, randomInt,
  getHashes() { return ["sha1", "sha256", "sha384", "sha512"]; },
};
export default crypto;
export { createHash, createHmac, randomBytes, timingSafeEqual, randomUUID, randomInt };
