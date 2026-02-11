// Minimal diagnostics_channel module stub for utoo-runtime

class Channel {
  constructor(name) {
    this.name = name;
    this._subscribers = [];
  }
  get hasSubscribers() { return this._subscribers.length > 0; }
  subscribe(fn) { this._subscribers.push(fn); }
  unsubscribe(fn) {
    const i = this._subscribers.indexOf(fn);
    if (i >= 0) { this._subscribers.splice(i, 1); return true; }
    return false;
  }
  publish(message) {
    for (const fn of this._subscribers) fn(message, this.name);
  }
  bindStore() {}
  unbindStore() {}
}

class TracingChannel {
  constructor(nameOrChannels) {
    const prefix = typeof nameOrChannels === "string" ? nameOrChannels : "";
    this.start = channel(prefix + ":start");
    this.end = channel(prefix + ":end");
    this.asyncStart = channel(prefix + ":asyncStart");
    this.asyncEnd = channel(prefix + ":asyncEnd");
    this.error = channel(prefix + ":error");
  }
  subscribe() {}
  unsubscribe() {}
  traceSync(fn, ctx) { return fn(ctx); }
  tracePromise(fn, ctx) { return fn(ctx); }
  traceCallback(fn, position, ctx, thisArg, ...args) { return fn.apply(thisArg, args); }
}

const _channels = new Map();

function channel(name) {
  if (_channels.has(name)) return _channels.get(name);
  const ch = new Channel(name);
  _channels.set(name, ch);
  return ch;
}

function hasSubscribers(name) {
  const ch = _channels.get(name);
  return ch ? ch.hasSubscribers : false;
}

function tracingChannel(nameOrChannels) {
  return new TracingChannel(nameOrChannels);
}

const dc = { channel, hasSubscribers, tracingChannel, Channel, TracingChannel };
dc.default = dc;

export default dc;
export { channel, hasSubscribers, tracingChannel, Channel, TracingChannel };
