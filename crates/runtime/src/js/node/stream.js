import EventEmitter from "ext:utoo_rt_ext/node/events";

// -- Stream (function-style for ES5 compat: Stream.call(this)) --
function Stream(opts) {
  EventEmitter.call(this);
  if (opts && typeof opts.read === "function") this._read = opts.read;
  if (opts && typeof opts.write === "function") this._write = opts.write;
}
Object.setPrototypeOf(Stream.prototype, EventEmitter.prototype);
Stream.prototype.constructor = Stream;

Stream.prototype.pipe = function (dest, opts) {
  var src = this;
  src.on("data", function (chunk) {
    var ret = dest.write(chunk);
    if (ret === false && src.pause) src.pause();
  });
  src.on("end", function () {
    if (!opts || opts.end !== false) dest.end();
  });
  if (typeof dest.on === "function") {
    dest.on("drain", function () { if (src.resume) src.resume(); });
  }
  if (typeof dest.emit === "function") {
    dest.emit("pipe", src);
  }
  return dest;
};

// -- Readable --
function Readable(opts) {
  if (!(this instanceof Readable)) return new Readable(opts);
  Stream.call(this, opts);
  this.readable = true;
  this._readableState = {
    flowing: null,
    ended: false,
    endEmitted: false,
    buffer: [],
    length: 0,
    highWaterMark: (opts && opts.highWaterMark) || 16384,
    objectMode: (opts && opts.objectMode) || false,
    encoding: null,
    destroyed: false,
  };
  if (opts && typeof opts.read === "function") this._read = opts.read;
}
Object.setPrototypeOf(Readable.prototype, Stream.prototype);
Readable.prototype.constructor = Readable;

Readable.prototype._read = function (_n) {};

Readable.prototype.read = function (_n) {
  var state = this._readableState;
  if (state.ended && state.buffer.length === 0) return null;
  if (state.buffer.length === 0 && !state.ended && !state._reading) {
    state._reading = true;
    try { this._read(state.highWaterMark); } catch (e) { this.destroy(e); }
    state._reading = false;
  }
  if (state.buffer.length > 0) {
    var chunk = state.buffer.shift();
    state.length -= chunk ? (chunk.length || 1) : 0;
    if (state.buffer.length === 0 && state.ended && !state.endEmitted) {
      state.endEmitted = true;
      this.emit("end");
    }
    return chunk;
  }
  return null;
};

Readable.prototype.push = function (chunk) {
  var state = this._readableState;
  if (chunk === null) {
    state.ended = true;
    if (state.flowing) {
      state.endEmitted = true;
      var self = this;
      queueMicrotask(function () { self.emit("end"); });
    }
    return false;
  }
  if (state.flowing) {
    this.emit("data", chunk);
  } else {
    state.buffer.push(chunk);
    state.length += chunk.length || 1;
  }
  return state.length < state.highWaterMark;
};

Readable.prototype.on = function (ev, fn) {
  EventEmitter.prototype.on.call(this, ev, fn);
  if (ev === "data" && this._readableState.flowing !== false) {
    this.resume();
  }
  return this;
};
Readable.prototype.addListener = Readable.prototype.on;

Readable.prototype.resume = function () {
  var state = this._readableState;
  if (!state.flowing) {
    state.flowing = true;
    while (state.buffer.length > 0) {
      var chunk = state.buffer.shift();
      state.length -= chunk ? (chunk.length || 1) : 0;
      this.emit("data", chunk);
    }
    if (!state.ended && !state._reading) {
      state._reading = true;
      try { this._read(state.highWaterMark); } catch (e) { this.destroy(e); }
      state._reading = false;
    }
    if (state.ended && !state.endEmitted) {
      state.endEmitted = true;
      var self = this;
      queueMicrotask(function () { self.emit("end"); });
    }
  }
  return this;
};

Readable.prototype.pause = function () {
  this._readableState.flowing = false;
  return this;
};

Readable.prototype.setEncoding = function (enc) {
  this._readableState.encoding = enc;
  return this;
};

Readable.prototype.isPaused = function () {
  return !this._readableState.flowing;
};

Readable.prototype.destroy = function (err) {
  var state = this._readableState;
  if (state.destroyed) return this;
  state.destroyed = true;
  if (err) this.emit("error", err);
  this.emit("close");
  return this;
};

Readable.prototype[Symbol.asyncIterator] = function () {
  var stream = this;
  var queue = [];
  var done = false;
  var resolve = null;
  stream.on("data", function (chunk) {
    if (resolve) { var r = resolve; resolve = null; r({ value: chunk, done: false }); }
    else queue.push(chunk);
  });
  stream.on("end", function () {
    done = true;
    if (resolve) { var r = resolve; resolve = null; r({ value: undefined, done: true }); }
  });
  stream.on("error", function (err) {
    done = true;
    if (resolve) { var r = resolve; resolve = null; r(Promise.reject(err)); }
  });
  return {
    next: function () {
      if (queue.length > 0) return Promise.resolve({ value: queue.shift(), done: false });
      if (done) return Promise.resolve({ value: undefined, done: true });
      return new Promise(function (r) { resolve = r; });
    },
    return: function () { stream.destroy(); return Promise.resolve({ value: undefined, done: true }); },
    [Symbol.asyncIterator]: function () { return this; },
  };
};

Readable.from = function (iterable, opts) {
  var readable = new Readable(opts);
  (async function () {
    for await (var chunk of iterable) readable.push(chunk);
    readable.push(null);
  })();
  return readable;
};

// -- Writable --
function Writable(opts) {
  if (!(this instanceof Writable)) return new Writable(opts);
  Stream.call(this, opts);
  this.writable = true;
  this._writableState = {
    ended: false,
    finished: false,
    destroyed: false,
    writing: false,
    buffer: [],
    length: 0,
    highWaterMark: (opts && opts.highWaterMark) || 16384,
    objectMode: (opts && opts.objectMode) || false,
    needDrain: false,
    corked: 0,
    finalCalled: false,
  };
  if (opts && typeof opts.write === "function") this._write = opts.write;
  if (opts && typeof opts.writev === "function") this._writev = opts.writev;
  if (opts && typeof opts.destroy === "function") this._destroy = opts.destroy;
  if (opts && typeof opts.final === "function") this._final = opts.final;
}
Object.setPrototypeOf(Writable.prototype, Stream.prototype);
Writable.prototype.constructor = Writable;

Writable.prototype._write = function (chunk, encoding, cb) { cb(); };

Writable.prototype.write = function (chunk, encoding, cb) {
  if (typeof encoding === "function") { cb = encoding; encoding = "utf8"; }
  var state = this._writableState;
  if (state.ended) {
    var err = new Error("write after end");
    if (cb) cb(err);
    return false;
  }
  var self = this;
  var onWrite = function (err) {
    state.writing = false;
    if (err) {
      if (cb) cb(err);
      self.emit("error", err);
      return;
    }
    if (cb) cb();
    if (state.needDrain) { state.needDrain = false; self.emit("drain"); }
    if (state.buffer.length > 0) {
      var next = state.buffer.shift();
      state.length -= next.chunk.length || 1;
      state.writing = true;
      self._write(next.chunk, next.encoding, onWrite);
    } else if (state.ended && !state.finished) {
      self._finish();
    }
  };
  if (state.writing || state.corked) {
    state.buffer.push({ chunk: chunk, encoding: encoding, cb: cb });
    state.length += chunk.length || 1;
    if (state.length >= state.highWaterMark) state.needDrain = true;
    return false;
  }
  state.writing = true;
  this._write(chunk, encoding, onWrite);
  return state.length < state.highWaterMark;
};

Writable.prototype.end = function (chunk, encoding, cb) {
  if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
  if (typeof encoding === "function") { cb = encoding; encoding = null; }
  var state = this._writableState;
  if (chunk != null) this.write(chunk, encoding);
  state.ended = true;
  if (cb) this.once("finish", cb);
  if (!state.writing && state.buffer.length === 0) this._finish();
  return this;
};

Writable.prototype._finish = function () {
  var state = this._writableState;
  if (state.finished) return;
  var self = this;
  var doFinish = function () {
    state.finished = true;
    self.emit("finish");
    self.emit("close");
  };
  if (this._final && !state.finalCalled) {
    state.finalCalled = true;
    this._final(doFinish);
  } else {
    doFinish();
  }
};

Writable.prototype.cork = function () { this._writableState.corked++; };

Writable.prototype.uncork = function () {
  var state = this._writableState;
  if (state.corked > 0) {
    state.corked--;
    if (state.corked === 0 && state.buffer.length > 0) {
      var next = state.buffer.shift();
      state.length -= next.chunk.length || 1;
      state.writing = true;
      var self = this;
      this._write(next.chunk, next.encoding, function (err) {
        state.writing = false;
        if (err) self.emit("error", err);
      });
    }
  }
};

Writable.prototype.destroy = function (err) {
  var state = this._writableState;
  if (state.destroyed) return this;
  state.destroyed = true;
  if (err) this.emit("error", err);
  this.emit("close");
  return this;
};

Writable.prototype.setDefaultEncoding = function (enc) {
  this._writableState.defaultEncoding = enc;
  return this;
};

// -- Duplex --
function Duplex(opts) {
  if (!(this instanceof Duplex)) return new Duplex(opts);
  Readable.call(this, opts);
  this.writable = true;
  this._writableState = {
    ended: false,
    finished: false,
    destroyed: false,
    writing: false,
    buffer: [],
    length: 0,
    highWaterMark: (opts && opts.highWaterMark) || 16384,
    objectMode: (opts && opts.objectMode) || false,
    needDrain: false,
    corked: 0,
    finalCalled: false,
  };
  if (opts && typeof opts.write === "function") this._write = opts.write;
  if (opts && typeof opts.writev === "function") this._writev = opts.writev;
  if (opts && typeof opts.final === "function") this._final = opts.final;
}
Object.setPrototypeOf(Duplex.prototype, Readable.prototype);
Duplex.prototype.constructor = Duplex;

// Mix in Writable methods
Duplex.prototype._write = Writable.prototype._write;
Duplex.prototype.write = Writable.prototype.write;
Duplex.prototype.end = Writable.prototype.end;
Duplex.prototype._finish = Writable.prototype._finish;
Duplex.prototype.cork = Writable.prototype.cork;
Duplex.prototype.uncork = Writable.prototype.uncork;
Duplex.prototype.setDefaultEncoding = Writable.prototype.setDefaultEncoding;

var _readableDestroy = Readable.prototype.destroy;
Duplex.prototype.destroy = function (err) {
  var ws = this._writableState;
  if (ws && !ws.destroyed) ws.destroyed = true;
  return _readableDestroy.call(this, err);
};

// -- Transform --
function Transform(opts) {
  if (!(this instanceof Transform)) return new Transform(opts);
  Duplex.call(this, opts);
  if (opts && typeof opts.transform === "function") this._transform = opts.transform;
  if (opts && typeof opts.flush === "function") this._flush = opts.flush;
}
Object.setPrototypeOf(Transform.prototype, Duplex.prototype);
Transform.prototype.constructor = Transform;

Transform.prototype._transform = function (chunk, _encoding, cb) { cb(null, chunk); };

Transform.prototype._write = function (chunk, encoding, cb) {
  this._transform(chunk, encoding, function (err, data) {
    if (err) return cb(err);
    if (data != null) this.push(data);
    cb();
  }.bind(this));
};

Transform.prototype._flush = function (cb) { cb(); };

Transform.prototype.end = function (chunk, encoding, cb) {
  if (typeof chunk === "function") { cb = chunk; chunk = null; }
  if (typeof encoding === "function") { cb = encoding; encoding = null; }
  if (chunk != null) this.write(chunk, encoding);
  var self = this;
  this._flush(function (err, data) {
    if (data != null) self.push(data);
    self.push(null);
    self._writableState.ended = true;
    self._writableState.finished = true;
    self.emit("finish");
    if (cb) cb(err);
  });
  return this;
};

// -- PassThrough --
function PassThrough(opts) {
  if (!(this instanceof PassThrough)) return new PassThrough(opts);
  Transform.call(this, opts);
}
Object.setPrototypeOf(PassThrough.prototype, Transform.prototype);
PassThrough.prototype.constructor = PassThrough;

PassThrough.prototype._transform = function (chunk, _encoding, cb) { cb(null, chunk); };

// -- Utility functions --
function pipeline() {
  var streams = Array.prototype.slice.call(arguments);
  var cb;
  if (typeof streams[streams.length - 1] === "function") cb = streams.pop();
  var error = null;
  for (var i = 0; i < streams.length - 1; i++) streams[i].pipe(streams[i + 1]);
  var last = streams[streams.length - 1];
  if (cb) {
    last.on("finish", function () { if (!error) cb(null); });
    for (var j = 0; j < streams.length; j++) {
      streams[j].on("error", function (err) { if (!error) { error = err; cb(err); } });
    }
  }
  return last;
}

function finished(stream, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = {}; }
  var cleanup = function () {
    stream.removeListener("finish", onFinish);
    stream.removeListener("error", onErr);
    stream.removeListener("end", onEnd);
    stream.removeListener("close", onClose);
  };
  var onFinish = function () { cleanup(); cb(null); };
  var onErr = function (err) { cleanup(); cb(err); };
  var onEnd = function () { cleanup(); cb(null); };
  var onClose = function () { cleanup(); cb(null); };
  stream.on("finish", onFinish);
  stream.on("error", onErr);
  stream.on("end", onEnd);
  stream.on("close", onClose);
  return cleanup;
}

// Node.js compat: require('stream') returns Stream with sub-classes as props
Stream.Stream = Stream;
Stream.Readable = Readable;
Stream.Writable = Writable;
Stream.Duplex = Duplex;
Stream.Transform = Transform;
Stream.PassThrough = PassThrough;
Stream.pipeline = pipeline;
Stream.finished = finished;
Stream.default = Stream;

export default Stream;
export { Stream, Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished };
