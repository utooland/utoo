class EventEmitter {
  constructor() {
    this._events = Object.create(null);
    this._maxListeners = EventEmitter.defaultMaxListeners;
  }

  on(event, listener) {
    if (!this._events[event]) this._events[event] = [];
    this._events[event].push(listener);
    return this;
  }

  addListener(event, listener) {
    return this.on(event, listener);
  }

  once(event, listener) {
    const wrapped = (...args) => {
      this.off(event, wrapped);
      listener.apply(this, args);
    };
    wrapped.listener = listener;
    return this.on(event, wrapped);
  }

  off(event, listener) {
    return this.removeListener(event, listener);
  }

  removeListener(event, listener) {
    const list = this._events[event];
    if (!list) return this;
    const idx = list.findIndex(
      (fn) => fn === listener || fn.listener === listener,
    );
    if (idx !== -1) list.splice(idx, 1);
    if (list.length === 0) delete this._events[event];
    return this;
  }

  removeAllListeners(event) {
    if (event) delete this._events[event];
    else this._events = Object.create(null);
    return this;
  }

  emit(event, ...args) {
    const list = this._events[event];
    if (!list || list.length === 0) {
      if (event === "error") {
        throw args[0] instanceof Error ? args[0] : new Error("Unhandled error");
      }
      return false;
    }
    for (const fn of list.slice()) fn.apply(this, args);
    return true;
  }

  listeners(event) {
    const list = this._events[event];
    if (!list) return [];
    return list.map((fn) => fn.listener || fn);
  }

  listenerCount(event) {
    const list = this._events[event];
    return list ? list.length : 0;
  }

  eventNames() {
    return Object.keys(this._events);
  }

  prependListener(event, listener) {
    if (!this._events[event]) this._events[event] = [];
    this._events[event].unshift(listener);
    return this;
  }

  prependOnceListener(event, listener) {
    const wrapped = (...args) => {
      this.off(event, wrapped);
      listener.apply(this, args);
    };
    wrapped.listener = listener;
    return this.prependListener(event, wrapped);
  }

  setMaxListeners(n) {
    this._maxListeners = n;
    return this;
  }

  getMaxListeners() {
    return this._maxListeners;
  }

  rawListeners(event) {
    return (this._events[event] || []).slice();
  }
}

EventEmitter.defaultMaxListeners = 10;
EventEmitter.EventEmitter = EventEmitter;

EventEmitter.listenerCount = function (emitter, type) {
  return emitter.listenerCount(type);
};

export default EventEmitter;
export { EventEmitter };
