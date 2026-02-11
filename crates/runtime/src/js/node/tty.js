// Minimal tty module stub for utoo-runtime
import { Readable, Writable } from "ext:utoo_rt_ext/node/stream";

class ReadStream extends Readable {
  constructor(fd) {
    super();
    this.fd = fd;
    this.isRaw = false;
    this.isTTY = false;
  }
  setRawMode(mode) { this.isRaw = mode; return this; }
}

class WriteStream extends Writable {
  constructor(fd) {
    super();
    this.fd = fd;
    this.isTTY = false;
    this.columns = 80;
    this.rows = 24;
  }
  clearLine() {}
  clearScreenDown() {}
  cursorTo() {}
  moveCursor() {}
  getColorDepth() { return 1; }
  hasColors() { return false; }
  getWindowSize() { return [this.columns, this.rows]; }
}

function isatty(fd) {
  return false;
}

const tty = { ReadStream, WriteStream, isatty };
tty.default = tty;

export default tty;
export { ReadStream, WriteStream, isatty };
