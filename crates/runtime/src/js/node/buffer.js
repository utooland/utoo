// Manual UTF-8 encode/decode to avoid dependency on TextEncoder/TextDecoder
// which may not be available in all V8 environments.
function utf8Encode(str) {
  const bytes = [];
  for (let i = 0; i < str.length; i++) {
    let code = str.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff && i + 1 < str.length) {
      const next = str.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        code = ((code - 0xd800) << 10) + (next - 0xdc00) + 0x10000;
        i++;
      }
    }
    if (code < 0x80) {
      bytes.push(code);
    } else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code < 0x10000) {
      bytes.push(
        0xe0 | (code >> 12),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    } else {
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return new Uint8Array(bytes);
}

function utf8Decode(bytes) {
  let str = "";
  let i = 0;
  while (i < bytes.length) {
    let code;
    if (bytes[i] < 0x80) {
      code = bytes[i++];
    } else if ((bytes[i] & 0xe0) === 0xc0) {
      code = ((bytes[i++] & 0x1f) << 6) | (bytes[i++] & 0x3f);
    } else if ((bytes[i] & 0xf0) === 0xe0) {
      code =
        ((bytes[i++] & 0x0f) << 12) |
        ((bytes[i++] & 0x3f) << 6) |
        (bytes[i++] & 0x3f);
    } else {
      code =
        ((bytes[i++] & 0x07) << 18) |
        ((bytes[i++] & 0x3f) << 12) |
        ((bytes[i++] & 0x3f) << 6) |
        (bytes[i++] & 0x3f);
      // Encode as surrogate pair
      code -= 0x10000;
      str += String.fromCharCode(0xd800 + (code >> 10), 0xdc00 + (code & 0x3ff));
      continue;
    }
    str += String.fromCharCode(code);
  }
  return str;
}

function latin1Encode(str) {
  const bytes = new Uint8Array(str.length);
  for (let i = 0; i < str.length; i++) {
    bytes[i] = str.charCodeAt(i) & 0xff;
  }
  return bytes;
}

function latin1Decode(bytes) {
  let str = "";
  for (let i = 0; i < bytes.length; i++) {
    str += String.fromCharCode(bytes[i]);
  }
  return str;
}

const SUPPORTED_ENCODINGS = [
  "utf8", "utf-8", "ascii", "latin1", "binary", "base64",
  "base64url", "hex", "ucs2", "ucs-2", "utf16le", "utf-16le",
];

// Internal class -- extends Uint8Array for real typed-array backing.
// Exported as `Buffer` via a function wrapper so that Buffer() works
// without `new` (required by many npm packages).
class _Buffer extends Uint8Array {
  static from(value, encodingOrOffset, length) {
    if (typeof value === "string") {
      const encoding = (encodingOrOffset || "utf-8").toLowerCase();
      if (encoding === "utf8" || encoding === "utf-8") {
        return new _Buffer(utf8Encode(value));
      }
      if (encoding === "latin1" || encoding === "binary") {
        return new _Buffer(latin1Encode(value));
      }
      if (encoding === "ascii") {
        const bytes = new Uint8Array(value.length);
        for (let i = 0; i < value.length; i++) {
          bytes[i] = value.charCodeAt(i) & 0x7f;
        }
        return new _Buffer(bytes);
      }
      if (encoding === "base64" || encoding === "base64url") {
        let str = value;
        if (encoding === "base64url") {
          str = str.replace(/-/g, "+").replace(/_/g, "/");
        }
        const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        str = str.replace(/=+$/, "");
        const bytes = [];
        for (let i = 0; i < str.length; i += 4) {
          const a = chars.indexOf(str[i]);
          const b = chars.indexOf(str[i + 1]);
          const c = chars.indexOf(str[i + 2]);
          const d = chars.indexOf(str[i + 3]);
          bytes.push((a << 2) | (b >> 4));
          if (c !== -1) bytes.push(((b & 0xf) << 4) | (c >> 2));
          if (d !== -1) bytes.push(((c & 0x3) << 6) | d);
        }
        return new _Buffer(new Uint8Array(bytes));
      }
      if (encoding === "hex") {
        const arr = new Uint8Array(value.length / 2);
        for (let i = 0; i < arr.length; i++) {
          arr[i] = parseInt(value.slice(i * 2, i * 2 + 2), 16);
        }
        return new _Buffer(arr);
      }
      if (encoding === "ucs2" || encoding === "ucs-2" ||
          encoding === "utf16le" || encoding === "utf-16le") {
        const bytes = new Uint8Array(value.length * 2);
        for (let i = 0; i < value.length; i++) {
          const code = value.charCodeAt(i);
          bytes[i * 2] = code & 0xff;
          bytes[i * 2 + 1] = (code >> 8) & 0xff;
        }
        return new _Buffer(bytes);
      }
      return new _Buffer(utf8Encode(value));
    }
    if (
      value instanceof ArrayBuffer ||
      (typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer)
    ) {
      return new _Buffer(new Uint8Array(value, encodingOrOffset, length));
    }
    if (ArrayBuffer.isView(value)) {
      return new _Buffer(
        new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
      );
    }
    if (Array.isArray(value)) {
      return new _Buffer(new Uint8Array(value));
    }
    return new _Buffer(new Uint8Array(value));
  }

  static alloc(size, fill, encoding) {
    const buf = new _Buffer(size);
    if (fill !== undefined) {
      if (typeof fill === "string") {
        const fillBuf = _Buffer.from(fill, encoding);
        for (let i = 0; i < size; i++) {
          buf[i] = fillBuf[i % fillBuf.length];
        }
      } else {
        const fillVal = typeof fill === "number" ? fill : 0;
        buf.fill(fillVal);
      }
    }
    return buf;
  }

  static allocUnsafe(size) {
    return new _Buffer(size);
  }

  static allocUnsafeSlow(size) {
    return new _Buffer(size);
  }

  static isBuffer(obj) {
    return obj instanceof _Buffer || (obj != null && obj._isBuffer === true);
  }

  static isEncoding(encoding) {
    if (typeof encoding !== "string") return false;
    return SUPPORTED_ENCODINGS.indexOf(encoding.toLowerCase()) !== -1;
  }

  static compare(a, b) {
    const len = Math.min(a.length, b.length);
    for (let i = 0; i < len; i++) {
      if (a[i] < b[i]) return -1;
      if (a[i] > b[i]) return 1;
    }
    if (a.length < b.length) return -1;
    if (a.length > b.length) return 1;
    return 0;
  }

  get _isBuffer() {
    return true;
  }

  static concat(list, totalLength) {
    if (totalLength === undefined) {
      totalLength = 0;
      for (const buf of list) totalLength += buf.length;
    }
    const result = _Buffer.alloc(totalLength);
    let offset = 0;
    for (const buf of list) {
      result.set(buf, offset);
      offset += buf.length;
      if (offset >= totalLength) break;
    }
    return result;
  }

  static byteLength(string, encoding) {
    if (typeof string !== "string") return string.byteLength || string.length;
    encoding = (encoding || "utf-8").toLowerCase();
    if (encoding === "utf8" || encoding === "utf-8") {
      return utf8Encode(string).length;
    }
    if (encoding === "latin1" || encoding === "binary" || encoding === "ascii") {
      return string.length;
    }
    if (encoding === "hex") {
      return string.length >>> 1;
    }
    if (encoding === "base64" || encoding === "base64url") {
      let len = string.length;
      if (string[len - 1] === "=") len--;
      if (string[len - 1] === "=") len--;
      return (len * 3) >>> 2;
    }
    if (encoding === "ucs2" || encoding === "ucs-2" ||
        encoding === "utf16le" || encoding === "utf-16le") {
      return string.length * 2;
    }
    return utf8Encode(string).length;
  }

  // -- toString with start/end support --
  toString(encoding, start, end) {
    encoding = (encoding || "utf-8").toLowerCase();
    start = start || 0;
    end = end === undefined ? this.length : end;
    if (start < 0) start = 0;
    if (end > this.length) end = this.length;
    if (end <= start) return "";

    const buf = start === 0 && end === this.length
      ? this : this.subarray(start, end);

    if (encoding === "utf8" || encoding === "utf-8") {
      return utf8Decode(buf);
    }
    if (encoding === "latin1" || encoding === "binary" || encoding === "ascii") {
      return latin1Decode(buf);
    }
    if (encoding === "base64") {
      const chars =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
      let result = "";
      for (let i = 0; i < buf.length; i += 3) {
        const a = buf[i];
        const b = i + 1 < buf.length ? buf[i + 1] : 0;
        const c = i + 2 < buf.length ? buf[i + 2] : 0;
        result += chars[a >> 2];
        result += chars[((a & 0x3) << 4) | (b >> 4)];
        result +=
          i + 1 < buf.length ? chars[((b & 0xf) << 2) | (c >> 6)] : "=";
        result += i + 2 < buf.length ? chars[c & 0x3f] : "=";
      }
      return result;
    }
    if (encoding === "hex") {
      let hex = "";
      for (let i = 0; i < buf.length; i++) {
        hex += buf[i].toString(16).padStart(2, "0");
      }
      return hex;
    }
    if (encoding === "ucs2" || encoding === "ucs-2" ||
        encoding === "utf16le" || encoding === "utf-16le") {
      let str = "";
      for (let i = 0; i + 1 < buf.length; i += 2) {
        str += String.fromCharCode(buf[i] | (buf[i + 1] << 8));
      }
      return str;
    }
    return utf8Decode(buf);
  }

  copy(target, targetStart, sourceStart, sourceEnd) {
    targetStart = targetStart || 0;
    sourceStart = sourceStart || 0;
    sourceEnd = sourceEnd || this.length;
    if (sourceEnd > this.length) sourceEnd = this.length;
    if (sourceStart >= sourceEnd) return 0;
    if (targetStart >= target.length) return 0;
    let bytesToCopy = sourceEnd - sourceStart;
    const available = target.length - targetStart;
    if (bytesToCopy > available) bytesToCopy = available;
    if (bytesToCopy <= 0) return 0;
    const slice = this.subarray(sourceStart, sourceStart + bytesToCopy);
    target.set(slice, targetStart);
    return bytesToCopy;
  }

  slice(start, end) {
    const sliced = super.subarray(start, end);
    return new _Buffer(sliced.buffer, sliced.byteOffset, sliced.byteLength);
  }

  subarray(start, end) {
    const sliced = super.subarray(start, end);
    return new _Buffer(sliced.buffer, sliced.byteOffset, sliced.byteLength);
  }

  // Node.js write(string[, offset[, length]][, encoding])
  write(string, offset, length, encoding) {
    if (typeof offset === "string") {
      encoding = offset;
      offset = 0;
      length = this.length;
    } else if (typeof length === "string") {
      encoding = length;
      length = this.length - (offset || 0);
    }
    offset = offset || 0;
    if (length === undefined) length = this.length - offset;
    encoding = (encoding || "utf-8").toLowerCase();

    let encoded;
    if (encoding === "utf8" || encoding === "utf-8") {
      encoded = utf8Encode(string);
    } else if (encoding === "latin1" || encoding === "binary" || encoding === "ascii") {
      encoded = latin1Encode(string);
    } else if (encoding === "hex") {
      encoded = new Uint8Array(string.length / 2);
      for (let i = 0; i < encoded.length; i++) {
        encoded[i] = parseInt(string.slice(i * 2, i * 2 + 2), 16);
      }
    } else {
      encoded = utf8Encode(string);
    }

    const len = Math.min(encoded.length, length, this.length - offset);
    this.set(encoded.subarray(0, len), offset);
    return len;
  }

  toJSON() {
    return { type: "Buffer", data: Array.from(this) };
  }

  equals(otherBuffer) {
    if (this.length !== otherBuffer.length) return false;
    for (let i = 0; i < this.length; i++) {
      if (this[i] !== otherBuffer[i]) return false;
    }
    return true;
  }

  compare(target, targetStart, targetEnd, sourceStart, sourceEnd) {
    targetStart = targetStart || 0;
    targetEnd = targetEnd === undefined ? target.length : targetEnd;
    sourceStart = sourceStart || 0;
    sourceEnd = sourceEnd === undefined ? this.length : sourceEnd;
    const src = this.subarray(sourceStart, sourceEnd);
    const tgt = target.subarray(targetStart, targetEnd);
    return _Buffer.compare(src, tgt);
  }

  indexOf(value, byteOffset, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = 0;
    }
    byteOffset = byteOffset || 0;

    if (typeof value === "number") {
      for (let i = byteOffset; i < this.length; i++) {
        if (this[i] === (value & 0xff)) return i;
      }
      return -1;
    }

    if (typeof value === "string") {
      value = _Buffer.from(value, encoding);
    }

    if (value.length === 0) return byteOffset;
    for (let i = byteOffset; i <= this.length - value.length; i++) {
      let found = true;
      for (let j = 0; j < value.length; j++) {
        if (this[i + j] !== value[j]) { found = false; break; }
      }
      if (found) return i;
    }
    return -1;
  }

  lastIndexOf(value, byteOffset, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = this.length - 1;
    }
    if (byteOffset === undefined) byteOffset = this.length - 1;

    if (typeof value === "number") {
      for (let i = byteOffset; i >= 0; i--) {
        if (this[i] === (value & 0xff)) return i;
      }
      return -1;
    }

    if (typeof value === "string") {
      value = _Buffer.from(value, encoding);
    }

    if (value.length === 0) return byteOffset;
    const maxStart = Math.min(byteOffset, this.length - value.length);
    for (let i = maxStart; i >= 0; i--) {
      let found = true;
      for (let j = 0; j < value.length; j++) {
        if (this[i + j] !== value[j]) { found = false; break; }
      }
      if (found) return i;
    }
    return -1;
  }

  includes(value, byteOffset, encoding) {
    return this.indexOf(value, byteOffset, encoding) !== -1;
  }

  swap16() {
    for (let i = 0; i < this.length; i += 2) {
      const tmp = this[i];
      this[i] = this[i + 1];
      this[i + 1] = tmp;
    }
    return this;
  }

  swap32() {
    for (let i = 0; i < this.length; i += 4) {
      const t0 = this[i]; const t1 = this[i + 1];
      this[i] = this[i + 3]; this[i + 1] = this[i + 2];
      this[i + 2] = t1; this[i + 3] = t0;
    }
    return this;
  }

  // -- Read methods --

  readUInt8(offset) {
    offset = offset >>> 0;
    return this[offset];
  }

  readUInt16LE(offset) {
    offset = offset >>> 0;
    return this[offset] | (this[offset + 1] << 8);
  }

  readUInt16BE(offset) {
    offset = offset >>> 0;
    return (this[offset] << 8) | this[offset + 1];
  }

  readUInt32LE(offset) {
    offset = offset >>> 0;
    return (this[offset] | (this[offset + 1] << 8) |
            (this[offset + 2] << 16)) + (this[offset + 3] * 0x1000000);
  }

  readUInt32BE(offset) {
    offset = offset >>> 0;
    return (this[offset] * 0x1000000) +
           ((this[offset + 1] << 16) | (this[offset + 2] << 8) | this[offset + 3]);
  }

  readInt8(offset) {
    offset = offset >>> 0;
    const val = this[offset];
    return val & 0x80 ? val - 0x100 : val;
  }

  readInt16LE(offset) {
    offset = offset >>> 0;
    const val = this[offset] | (this[offset + 1] << 8);
    return val & 0x8000 ? val - 0x10000 : val;
  }

  readInt16BE(offset) {
    offset = offset >>> 0;
    const val = (this[offset] << 8) | this[offset + 1];
    return val & 0x8000 ? val - 0x10000 : val;
  }

  readInt32LE(offset) {
    offset = offset >>> 0;
    return this[offset] | (this[offset + 1] << 8) |
           (this[offset + 2] << 16) | (this[offset + 3] << 24);
  }

  readInt32BE(offset) {
    offset = offset >>> 0;
    return (this[offset] << 24) | (this[offset + 1] << 16) |
           (this[offset + 2] << 8) | this[offset + 3];
  }

  readFloatLE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getFloat32(offset, true);
  }

  readFloatBE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getFloat32(offset, false);
  }

  readDoubleLE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getFloat64(offset, true);
  }

  readDoubleBE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getFloat64(offset, false);
  }

  readBigUInt64LE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getBigUint64(offset, true);
  }

  readBigUInt64BE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getBigUint64(offset, false);
  }

  readBigInt64LE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getBigInt64(offset, true);
  }

  readBigInt64BE(offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return dv.getBigInt64(offset, false);
  }

  readUIntLE(offset, byteLength) {
    offset = offset >>> 0;
    let val = 0;
    let mul = 1;
    for (let i = 0; i < byteLength; i++) {
      val += this[offset + i] * mul;
      mul *= 0x100;
    }
    return val;
  }

  readUIntBE(offset, byteLength) {
    offset = offset >>> 0;
    let val = 0;
    let mul = 1;
    for (let i = byteLength - 1; i >= 0; i--) {
      val += this[offset + i] * mul;
      mul *= 0x100;
    }
    return val;
  }

  readIntLE(offset, byteLength) {
    offset = offset >>> 0;
    let val = 0;
    let mul = 1;
    for (let i = 0; i < byteLength; i++) {
      val += this[offset + i] * mul;
      mul *= 0x100;
    }
    if (val >= mul / 2) val -= mul;
    return val;
  }

  readIntBE(offset, byteLength) {
    offset = offset >>> 0;
    let val = 0;
    let mul = 1;
    for (let i = byteLength - 1; i >= 0; i--) {
      val += this[offset + i] * mul;
      mul *= 0x100;
    }
    if (val >= mul / 2) val -= mul;
    return val;
  }

  // -- Write methods --

  writeUInt8(value, offset) {
    offset = offset >>> 0;
    this[offset] = value & 0xff;
    return offset + 1;
  }

  writeUInt16LE(value, offset) {
    offset = offset >>> 0;
    this[offset] = value & 0xff;
    this[offset + 1] = (value >>> 8) & 0xff;
    return offset + 2;
  }

  writeUInt16BE(value, offset) {
    offset = offset >>> 0;
    this[offset] = (value >>> 8) & 0xff;
    this[offset + 1] = value & 0xff;
    return offset + 2;
  }

  writeUInt32LE(value, offset) {
    offset = offset >>> 0;
    this[offset] = value & 0xff;
    this[offset + 1] = (value >>> 8) & 0xff;
    this[offset + 2] = (value >>> 16) & 0xff;
    this[offset + 3] = (value >>> 24) & 0xff;
    return offset + 4;
  }

  writeUInt32BE(value, offset) {
    offset = offset >>> 0;
    this[offset] = (value >>> 24) & 0xff;
    this[offset + 1] = (value >>> 16) & 0xff;
    this[offset + 2] = (value >>> 8) & 0xff;
    this[offset + 3] = value & 0xff;
    return offset + 4;
  }

  writeInt8(value, offset) {
    offset = offset >>> 0;
    if (value < 0) value = 0x100 + value;
    this[offset] = value & 0xff;
    return offset + 1;
  }

  writeInt16LE(value, offset) {
    offset = offset >>> 0;
    this[offset] = value & 0xff;
    this[offset + 1] = (value >> 8) & 0xff;
    return offset + 2;
  }

  writeInt16BE(value, offset) {
    offset = offset >>> 0;
    this[offset] = (value >> 8) & 0xff;
    this[offset + 1] = value & 0xff;
    return offset + 2;
  }

  writeInt32LE(value, offset) {
    offset = offset >>> 0;
    this[offset] = value & 0xff;
    this[offset + 1] = (value >> 8) & 0xff;
    this[offset + 2] = (value >> 16) & 0xff;
    this[offset + 3] = (value >> 24) & 0xff;
    return offset + 4;
  }

  writeInt32BE(value, offset) {
    offset = offset >>> 0;
    this[offset] = (value >> 24) & 0xff;
    this[offset + 1] = (value >> 16) & 0xff;
    this[offset + 2] = (value >> 8) & 0xff;
    this[offset + 3] = value & 0xff;
    return offset + 4;
  }

  writeFloatLE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setFloat32(offset, value, true);
    return offset + 4;
  }

  writeFloatBE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setFloat32(offset, value, false);
    return offset + 4;
  }

  writeDoubleLE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setFloat64(offset, value, true);
    return offset + 8;
  }

  writeDoubleBE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setFloat64(offset, value, false);
    return offset + 8;
  }

  writeBigUInt64LE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setBigUint64(offset, value, true);
    return offset + 8;
  }

  writeBigUInt64BE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setBigUint64(offset, value, false);
    return offset + 8;
  }

  writeBigInt64LE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setBigInt64(offset, value, true);
    return offset + 8;
  }

  writeBigInt64BE(value, offset) {
    offset = offset >>> 0;
    const dv = new DataView(this.buffer, this.byteOffset, this.byteLength);
    dv.setBigInt64(offset, value, false);
    return offset + 8;
  }

  writeUIntLE(value, offset, byteLength) {
    offset = offset >>> 0;
    let mul = 1;
    for (let i = 0; i < byteLength; i++) {
      this[offset + i] = (value / mul) & 0xff;
      mul *= 0x100;
    }
    return offset + byteLength;
  }

  writeUIntBE(value, offset, byteLength) {
    offset = offset >>> 0;
    let mul = 1;
    for (let i = byteLength - 1; i >= 0; i--) {
      this[offset + i] = (value / mul) & 0xff;
      mul *= 0x100;
    }
    return offset + byteLength;
  }

  writeIntLE(value, offset, byteLength) {
    offset = offset >>> 0;
    let mul = 1;
    for (let i = 0; i < byteLength; i++) {
      this[offset + i] = (value / mul) & 0xff;
      mul *= 0x100;
    }
    return offset + byteLength;
  }

  writeIntBE(value, offset, byteLength) {
    offset = offset >>> 0;
    let mul = 1;
    for (let i = byteLength - 1; i >= 0; i--) {
      this[offset + i] = (value / mul) & 0xff;
      mul *= 0x100;
    }
    return offset + byteLength;
  }
}

// Make prototype methods enumerable so that `for (op in Buffer.prototype)`
// works. Node.js native Buffer has enumerable prototype methods; many npm
// packages (e.g. mysql2 Packet.MockBuffer) rely on this for iteration.
for (const key of Object.getOwnPropertyNames(_Buffer.prototype)) {
  if (key === "constructor") continue;
  const desc = Object.getOwnPropertyDescriptor(_Buffer.prototype, key);
  if (desc && typeof desc.value === "function") {
    Object.defineProperty(_Buffer.prototype, key, { ...desc, enumerable: true });
  }
}

// -- Buffer function wrapper --
// Many npm packages call Buffer() without new (deprecated but widespread).
// A class cannot be called without new, so we wrap it in a function.
function Buffer(arg, encodingOrOffset, length) {
  if (typeof arg === "number") {
    return new _Buffer(arg);
  }
  return _Buffer.from(arg, encodingOrOffset, length);
}

// Wire up prototype so instanceof works
Buffer.prototype = _Buffer.prototype;
_Buffer.prototype.constructor = Buffer;

// Copy static methods
Buffer.from = function(value, encodingOrOffset, length) {
  return _Buffer.from(value, encodingOrOffset, length);
};
Buffer.alloc = function(size, fill, encoding) {
  return _Buffer.alloc(size, fill, encoding);
};
Buffer.allocUnsafe = function(size) { return _Buffer.allocUnsafe(size); };
Buffer.allocUnsafeSlow = function(size) { return _Buffer.allocUnsafeSlow(size); };
Buffer.isBuffer = function(obj) { return _Buffer.isBuffer(obj); };
Buffer.isEncoding = function(enc) { return _Buffer.isEncoding(enc); };
Buffer.compare = function(a, b) { return _Buffer.compare(a, b); };
Buffer.concat = function(list, totalLength) { return _Buffer.concat(list, totalLength); };
Buffer.byteLength = function(string, encoding) { return _Buffer.byteLength(string, encoding); };

Object.defineProperty(Buffer, Symbol.hasInstance, {
  value: function(instance) { return instance instanceof _Buffer; },
});

// Re-export Blob and File from globals (Node.js buffer module exports these)
const Blob = globalThis.Blob;
const File = globalThis.File;

const kMaxLength = 2 ** 31 - 1;
const kStringMaxLength = 2 ** 28 - 16;
const SlowBuffer = Buffer;

const constants = { MAX_LENGTH: kMaxLength, MAX_STRING_LENGTH: kStringMaxLength };

export { Buffer, Blob, File, SlowBuffer, kMaxLength, kStringMaxLength, constants };
export default { Buffer, Blob, File, SlowBuffer, kMaxLength, kStringMaxLength, constants };
