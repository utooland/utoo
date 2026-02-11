function EventEmitter() {
  if (!(this instanceof EventEmitter)) {
    return new EventEmitter();
  }
  this._events = Object.create(null);
  this._maxListeners = EventEmitter.defaultMaxListeners;
}

EventEmitter.prototype.on = function (event, listener) {
  if (!this._events[event]) this._events[event] = [];
  this._events[event].push(listener);
  return this;
};

EventEmitter.prototype.addListener = EventEmitter.prototype.on;

EventEmitter.prototype.once = function (event, listener) {
  const self = this;
  function wrapped(...args) {
    self.off(event, wrapped);
    listener.apply(self, args);
  }
  wrapped.listener = listener;
  return this.on(event, wrapped);
};

EventEmitter.prototype.off = function (event, listener) {
  return this.removeListener(event, listener);
};

EventEmitter.prototype.removeListener = function (event, listener) {
  const list = this._events[event];
  if (!list) return this;
  const idx = list.findIndex(
    (fn) => fn === listener || fn.listener === listener,
  );
  if (idx !== -1) list.splice(idx, 1);
  if (list.length === 0) delete this._events[event];
  return this;
};

EventEmitter.prototype.removeAllListeners = function (event) {
  if (event) delete this._events[event];
  else this._events = Object.create(null);
  return this;
};

EventEmitter.prototype.emit = function (event, ...args) {
  const list = this._events[event];
  if (!list || list.length === 0) {
    if (event === "error") {
      throw args[0] instanceof Error ? args[0] : new Error("Unhandled error");
    }
    return false;
  }
  for (const fn of list.slice()) fn.apply(this, args);
  return true;
};

EventEmitter.prototype.listeners = function (event) {
  const list = this._events[event];
  if (!list) return [];
  return list.map((fn) => fn.listener || fn);
};

EventEmitter.prototype.listenerCount = function (event) {
  const list = this._events[event];
  return list ? list.length : 0;
};

EventEmitter.prototype.eventNames = function () {
  return Object.keys(this._events);
};

EventEmitter.prototype.prependListener = function (event, listener) {
  if (!this._events[event]) this._events[event] = [];
  this._events[event].unshift(listener);
  return this;
};

EventEmitter.prototype.prependOnceListener = function (event, listener) {
  const self = this;
  function wrapped(...args) {
    self.off(event, wrapped);
    listener.apply(self, args);
  }
  wrapped.listener = listener;
  return this.prependListener(event, wrapped);
};

EventEmitter.prototype.setMaxListeners = function (n) {
  this._maxListeners = n;
  return this;
};

EventEmitter.prototype.getMaxListeners = function () {
  return this._maxListeners;
};

EventEmitter.prototype.rawListeners = function (event) {
  return (this._events[event] || []).slice();
};

EventEmitter.defaultMaxListeners = 10;
EventEmitter.EventEmitter = EventEmitter;

EventEmitter.listenerCount = function (emitter, type) {
  return emitter.listenerCount(type);
};

export default EventEmitter;
export { EventEmitter };
