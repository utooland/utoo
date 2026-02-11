// node:console module for utoo-runtime
// Re-export the global console object

class Console {
  constructor(stdout, stderr) {
    this._stdout = stdout || globalThis.process?.stdout;
    this._stderr = stderr || globalThis.process?.stderr;
  }
  log(...args) { globalThis.console.log(...args); }
  warn(...args) { globalThis.console.warn(...args); }
  error(...args) { globalThis.console.error(...args); }
  info(...args) { globalThis.console.info(...args); }
  debug(...args) { globalThis.console.debug(...args); }
  dir(obj) { globalThis.console.log(obj); }
  time() {}
  timeEnd() {}
  timeLog() {}
  trace() { globalThis.console.log(new Error().stack); }
  assert(val, ...args) { if (!val) globalThis.console.error("Assertion failed:", ...args); }
  count() {}
  countReset() {}
  group() {}
  groupCollapsed() {}
  groupEnd() {}
  table() {}
  clear() {}
}

const consoleModule = new Console();
consoleModule.Console = Console;
consoleModule.default = consoleModule;

export default consoleModule;
export { Console };
