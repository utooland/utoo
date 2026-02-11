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

class Buffer extends Uint8Array {
  static from(value, encodingOrOffset, length) {
    if (typeof value === "string") {
      const encoding = encodingOrOffset || "utf-8";
      if (encoding === "utf8" || encoding === "utf-8") {
        return new Buffer(utf8Encode(value));
      }
      if (encoding === "base64") {
        // Manual base64 decode
        const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let str = value.replace(/=+$/, "");
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
        return new Buffer(new Uint8Array(bytes));
      }
      if (encoding === "hex") {
        const arr = new Uint8Array(value.length / 2);
        for (let i = 0; i < arr.length; i++) {
          arr[i] = parseInt(value.slice(i * 2, i * 2 + 2), 16);
        }
        return new Buffer(arr);
      }
      return new Buffer(utf8Encode(value));
    }
    if (value instanceof ArrayBuffer || value instanceof SharedArrayBuffer) {
      return new Buffer(new Uint8Array(value, encodingOrOffset, length));
    }
    if (ArrayBuffer.isView(value)) {
      return new Buffer(
        new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
      );
    }
    if (Array.isArray(value)) {
      return new Buffer(new Uint8Array(value));
    }
    return new Buffer(new Uint8Array(value));
  }

  static alloc(size, fill, _encoding) {
    const buf = new Buffer(size);
    if (fill !== undefined) {
      const fillVal = typeof fill === "number" ? fill : 0;
      buf.fill(fillVal);
    }
    return buf;
  }

  static allocUnsafe(size) {
    return new Buffer(size);
  }

  static isBuffer(obj) {
    return obj instanceof Buffer;
  }

  static concat(list, totalLength) {
    if (totalLength === undefined) {
      totalLength = 0;
      for (const buf of list) totalLength += buf.length;
    }
    const result = Buffer.alloc(totalLength);
    let offset = 0;
    for (const buf of list) {
      result.set(buf, offset);
      offset += buf.length;
      if (offset >= totalLength) break;
    }
    return result;
  }

  static byteLength(string, _encoding) {
    if (typeof string !== "string") return string.byteLength || string.length;
    return utf8Encode(string).length;
  }

  toString(encoding) {
    encoding = encoding || "utf-8";
    if (encoding === "utf8" || encoding === "utf-8") {
      return utf8Decode(this);
    }
    if (encoding === "base64") {
      const chars =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
      let result = "";
      for (let i = 0; i < this.length; i += 3) {
        const a = this[i];
        const b = i + 1 < this.length ? this[i + 1] : 0;
        const c = i + 2 < this.length ? this[i + 2] : 0;
        result += chars[a >> 2];
        result += chars[((a & 0x3) << 4) | (b >> 4)];
        result +=
          i + 1 < this.length ? chars[((b & 0xf) << 2) | (c >> 6)] : "=";
        result += i + 2 < this.length ? chars[c & 0x3f] : "=";
      }
      return result;
    }
    if (encoding === "hex") {
      let hex = "";
      for (let i = 0; i < this.length; i++) {
        hex += this[i].toString(16).padStart(2, "0");
      }
      return hex;
    }
    return utf8Decode(this);
  }

  copy(target, targetStart, sourceStart, sourceEnd) {
    targetStart = targetStart || 0;
    sourceStart = sourceStart || 0;
    sourceEnd = sourceEnd || this.length;
    const slice = this.subarray(sourceStart, sourceEnd);
    target.set(slice, targetStart);
    return slice.length;
  }

  slice(start, end) {
    const sliced = super.subarray(start, end);
    return new Buffer(sliced.buffer, sliced.byteOffset, sliced.byteLength);
  }

  write(string, offset, length, _encoding) {
    offset = offset || 0;
    const encoded = utf8Encode(string);
    const len = Math.min(
      encoded.length,
      length !== undefined ? length : this.length - offset,
      this.length - offset,
    );
    this.set(encoded.subarray(0, len), offset);
    return len;
  }

  toJSON() {
    return { type: "Buffer", data: Array.from(this) };
  }
}

export { Buffer };
export default { Buffer };
