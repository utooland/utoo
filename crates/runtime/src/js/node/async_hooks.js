class AsyncLocalStorage {
  constructor() {
    this._store = undefined;
  }
  getStore() { return this._store; }
  run(store, callback, ...args) {
    const prev = this._store;
    this._store = store;
    try { return callback(...args); }
    finally { this._store = prev; }
  }
  enterWith(store) { this._store = store; }
  disable() { this._store = undefined; }
}

class AsyncResource {
  constructor(type, opts) {
    this.type = type;
  }
  runInAsyncScope(fn, thisArg, ...args) { return fn.call(thisArg, ...args); }
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
