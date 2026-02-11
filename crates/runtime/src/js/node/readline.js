// Minimal readline module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

class Interface extends EventEmitter {
  constructor(input, output, completer, terminal) {
    super();
    this.input = input;
    this.output = output;
    this.terminal = terminal || false;
    this.line = "";
    this.cursor = 0;
    this.closed = false;
  }
  question(query, opts, cb) {
    if (typeof opts === "function") { cb = opts; }
    if (cb) cb("");
  }
  close() {
    this.closed = true;
    this.emit("close");
  }
  pause() { return this; }
  resume() { return this; }
  write() {}
  setPrompt() {}
  prompt() {}
  getPrompt() { return ""; }
}

function createInterface(opts) {
  if (typeof opts === "object" && !opts.input) {
    return new Interface(opts.input, opts.output, opts.completer, opts.terminal);
  }
  return new Interface(arguments[0], arguments[1], arguments[2], arguments[3]);
}

function clearLine() {}
function clearScreenDown() {}
function cursorTo() {}
function moveCursor() {}
function emitKeypressEvents() {}

const readline = {
  Interface, createInterface,
  clearLine, clearScreenDown, cursorTo, moveCursor, emitKeypressEvents,
};
readline.default = readline;

export default readline;
export { Interface, createInterface, clearLine, clearScreenDown, cursorTo, moveCursor, emitKeypressEvents };
