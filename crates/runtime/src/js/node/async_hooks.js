// AsyncLocalStorage backed by deno_core's AsyncVariable (V8 async context tracking)
const { AsyncVariable, getAsyncContext, setAsyncContext } = Deno.core;

class AsyncLocalStorage {
  constructor() {
    this._variable = new AsyncVariable();
  }
  getStore() { return this._variable.get(); }
  run(store, callback, ...args) {
    // enter() returns the previous full async context snapshot,
    // so we must restore with setAsyncContext(), not enter(prev)
    const prev = this._variable.enter(store);
    try {
      return callback(...args);
    } finally {
      setAsyncContext(prev);
    }
  }
  exit(callback, ...args) {
    return this.run(undefined, callback, ...args);
  }
  enterWith(store) { this._variable.enter(store); }
  disable() { this._variable.enter(undefined); }
  static bind(fn) {
    const ctx = getAsyncContext();
    return function(...args) {
      const prev = setAsyncContext(ctx);
      try {
        return fn(...args);
      } finally {
        setAsyncContext(prev);
      }
    };
  }
  static snapshot() {
    const ctx = getAsyncContext();
    return function(fn, ...args) {
      const prev = setAsyncContext(ctx);
      try {
        return fn(...args);
      } finally {
        setAsyncContext(prev);
      }
    };
  }
}

class AsyncResource {
  constructor(type, opts) {
    this.type = type;
    this._ctx = getAsyncContext();
  }
  runInAsyncScope(fn, thisArg, ...args) {
    const prev = setAsyncContext(this._ctx);
    try {
      return fn.call(thisArg, ...args);
    } finally {
      setAsyncContext(prev);
    }
  }
  emitDestroy() { return this; }
  asyncId() { return -1; }
  triggerAsyncId() { return -1; }
}

function executionAsyncId() { return -1; }
function triggerAsyncId() { return -1; }
function createHook() { return { enable() {}, disable() {} }; }

const async_hooks = { AsyncLocalStorage, AsyncResource, executionAsyncId, triggerAsyncId, createHook };
export default async_hooks;
export { AsyncLocalStorage, AsyncResource, executionAsyncId, triggerAsyncId, createHook };
