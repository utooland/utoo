// node:timers module for utoo-runtime

const _setTimeout = globalThis.setTimeout;
const _setInterval = globalThis.setInterval;
const _clearTimeout = globalThis.clearTimeout;
const _clearInterval = globalThis.clearInterval;
const _setImmediate = globalThis.setImmediate;
const _clearImmediate = globalThis.clearImmediate;

const promises = {
  setTimeout(ms, value) {
    return new Promise((resolve) => _setTimeout(() => resolve(value), ms));
  },
  setInterval(ms, value) {
    // Returns an async iterator - simplified stub
    throw new Error("timers/promises.setInterval not implemented");
  },
  setImmediate(value) {
    return new Promise((resolve) => _setImmediate(() => resolve(value)));
  },
};

const timers = {
  setTimeout: _setTimeout,
  setInterval: _setInterval,
  clearTimeout: _clearTimeout,
  clearInterval: _clearInterval,
  setImmediate: _setImmediate,
  clearImmediate: _clearImmediate,
  promises,
};
timers.default = timers;

export default timers;
export {
  _setTimeout as setTimeout,
  _setInterval as setInterval,
  _clearTimeout as clearTimeout,
  _clearInterval as clearInterval,
  _setImmediate as setImmediate,
  _clearImmediate as clearImmediate,
  promises,
};
