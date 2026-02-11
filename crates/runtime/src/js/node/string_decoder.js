class StringDecoder {
  constructor(encoding) {
    this.encoding = (encoding || "utf8").toLowerCase().replace("-", "");
    this._buffer = new Uint8Array(0);
  }

  write(buf) {
    if (typeof buf === "string") return buf;
    if (!(buf instanceof Uint8Array)) buf = new Uint8Array(buf);

    if (this._buffer.length > 0) {
      const merged = new Uint8Array(this._buffer.length + buf.length);
      merged.set(this._buffer);
      merged.set(buf, this._buffer.length);
      buf = merged;
      this._buffer = new Uint8Array(0);
    }

    if (this.encoding === "utf8") {
      let end = buf.length;
      if (end > 0) {
        const last = buf[end - 1];
        if (last >= 0xc0) {
          this._buffer = buf.slice(end - 1);
          end -= 1;
        } else if (end >= 2) {
          const prev = buf[end - 2];
          if (prev >= 0xe0 && last >= 0x80 && last <= 0xbf) {
            const needed = prev >= 0xf0 ? 4 : 3;
            if (2 < needed) {
              this._buffer = buf.slice(end - 2);
              end -= 2;
            }
          }
        }
      }
      return new TextDecoder("utf-8").decode(buf.subarray(0, end));
    }

    return new TextDecoder("utf-8").decode(buf);
  }

  end(buf) {
    let result = "";
    if (buf) result = this.write(buf);
    if (this._buffer.length > 0) {
      result += new TextDecoder("utf-8").decode(this._buffer);
      this._buffer = new Uint8Array(0);
    }
    return result;
  }
}

const sd = { StringDecoder };
export default sd;
export { StringDecoder };
