import EventEmitter from "ext:utoo_rt_ext/node/events";

class Stream extends EventEmitter {
  constructor(opts) {
    super();
    if (opts && typeof opts.read === "function") this._read = opts.read;
    if (opts && typeof opts.write === "function") this._write = opts.write;
  }

  pipe(dest, opts) {
    const src = this;
    src.on("data", (chunk) => {
      const ret = dest.write(chunk);
      if (ret === false && src.pause) src.pause();
    });
    src.on("end", () => {
      if (!opts || opts.end !== false) dest.end();
    });
    dest.on("drain", () => { if (src.resume) src.resume(); });
    dest.emit("pipe", src);
    return dest;
  }
}

class Readable extends Stream {
  constructor(opts) {
    super(opts);
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

  _read(_n) {}

  read(_n) {
    const state = this._readableState;
    if (state.ended && state.buffer.length === 0) return null;
    if (state.buffer.length > 0) {
      const chunk = state.buffer.shift();
      state.length -= chunk ? (chunk.length || 1) : 0;
      if (state.buffer.length === 0 && state.ended && !state.endEmitted) {
        state.endEmitted = true;
        this.emit("end");
      }
      return chunk;
    }
    return null;
  }

  push(chunk) {
    const state = this._readableState;
    if (chunk === null) {
      state.ended = true;
      if (state.flowing) {
        state.endEmitted = true;
        queueMicrotask(() => this.emit("end"));
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
  }

  resume() {
    const state = this._readableState;
    if (!state.flowing) {
      state.flowing = true;
      while (state.buffer.length > 0) {
        const chunk = state.buffer.shift();
        state.length -= chunk ? (chunk.length || 1) : 0;
        this.emit("data", chunk);
      }
      if (state.ended && !state.endEmitted) {
        state.endEmitted = true;
        queueMicrotask(() => this.emit("end"));
      }
    }
    return this;
  }

  pause() {
    this._readableState.flowing = false;
    return this;
  }

  setEncoding(enc) {
    this._readableState.encoding = enc;
    return this;
  }

  isPaused() {
    return !this._readableState.flowing;
  }

  on(ev, fn) {
    super.on(ev, fn);
    if (ev === "data" && this._readableState.flowing !== false) this.resume();
    return this;
  }

  destroy(err) {
    const state = this._readableState;
    if (state.destroyed) return this;
    state.destroyed = true;
    if (err) this.emit("error", err);
    this.emit("close");
    return this;
  }

  [Symbol.asyncIterator]() {
    const stream = this;
    const queue = [];
    let done = false;
    let resolve = null;
    stream.on("data", (chunk) => {
      if (resolve) { const r = resolve; resolve = null; r({ value: chunk, done: false }); }
      else queue.push(chunk);
    });
    stream.on("end", () => {
      done = true;
      if (resolve) { const r = resolve; resolve = null; r({ value: undefined, done: true }); }
    });
    stream.on("error", (err) => {
      done = true;
      if (resolve) { const r = resolve; resolve = null; r(Promise.reject(err)); }
    });
    return {
      next() {
        if (queue.length > 0) return Promise.resolve({ value: queue.shift(), done: false });
        if (done) return Promise.resolve({ value: undefined, done: true });
        return new Promise((r) => { resolve = r; });
      },
      return() { stream.destroy(); return Promise.resolve({ value: undefined, done: true }); },
      [Symbol.asyncIterator]() { return this; },
    };
  }
}

Readable.from = function (iterable, opts) {
  const readable = new Readable(opts);
  (async () => {
    for await (const chunk of iterable) readable.push(chunk);
    readable.push(null);
  })();
  return readable;
};

class Writable extends Stream {
  constructor(opts) {
    super(opts);
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

  _write(chunk, encoding, cb) { cb(); }

  write(chunk, encoding, cb) {
    if (typeof encoding === "function") { cb = encoding; encoding = "utf8"; }
    const state = this._writableState;
    if (state.ended) {
      const err = new Error("write after end");
      if (cb) cb(err);
      return false;
    }
    const onWrite = (err) => {
      state.writing = false;
      if (err) {
        if (cb) cb(err);
        this.emit("error", err);
        return;
      }
      if (cb) cb();
      if (state.needDrain) { state.needDrain = false; this.emit("drain"); }
      if (state.buffer.length > 0) {
        const next = state.buffer.shift();
        state.length -= next.chunk.length || 1;
        state.writing = true;
        this._write(next.chunk, next.encoding, onWrite);
      } else if (state.ended && !state.finished) {
        this._finish();
      }
    };
    if (state.writing || state.corked) {
      state.buffer.push({ chunk, encoding, cb });
      state.length += chunk.length || 1;
      if (state.length >= state.highWaterMark) state.needDrain = true;
      return false;
    }
    state.writing = true;
    this._write(chunk, encoding, onWrite);
    return state.length < state.highWaterMark;
  }

  end(chunk, encoding, cb) {
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    const state = this._writableState;
    if (chunk != null) this.write(chunk, encoding);
    state.ended = true;
    if (cb) this.once("finish", cb);
    if (!state.writing && state.buffer.length === 0) this._finish();
    return this;
  }

  _finish() {
    const state = this._writableState;
    if (state.finished) return;
    const doFinish = () => {
      state.finished = true;
      this.emit("finish");
      this.emit("close");
    };
    if (this._final && !state.finalCalled) {
      state.finalCalled = true;
      this._final(doFinish);
    } else {
      doFinish();
    }
  }

  cork() { this._writableState.corked++; }

  uncork() {
    const state = this._writableState;
    if (state.corked > 0) {
      state.corked--;
      if (state.corked === 0 && state.buffer.length > 0) {
        const next = state.buffer.shift();
        state.length -= next.chunk.length || 1;
        state.writing = true;
        this._write(next.chunk, next.encoding, (err) => {
          state.writing = false;
          if (err) this.emit("error", err);
        });
      }
    }
  }

  destroy(err) {
    const state = this._writableState;
    if (state.destroyed) return this;
    state.destroyed = true;
    if (err) this.emit("error", err);
    this.emit("close");
    return this;
  }

  setDefaultEncoding(enc) {
    this._writableState.defaultEncoding = enc;
    return this;
  }
}

class Duplex extends Readable {
  constructor(opts) {
    super(opts);
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
}

// Mix in Writable methods
for (const method of ["_write", "write", "end", "_finish", "cork", "uncork", "setDefaultEncoding"]) {
  Duplex.prototype[method] = Writable.prototype[method];
}

const _readableDestroy = Readable.prototype.destroy;
Duplex.prototype.destroy = function (err) {
  const ws = this._writableState;
  if (ws && !ws.destroyed) ws.destroyed = true;
  return _readableDestroy.call(this, err);
};

class Transform extends Duplex {
  constructor(opts) {
    super(opts);
    if (opts && typeof opts.transform === "function") this._transform = opts.transform;
    if (opts && typeof opts.flush === "function") this._flush = opts.flush;
  }

  _transform(chunk, _encoding, cb) { cb(null, chunk); }

  _write(chunk, encoding, cb) {
    this._transform(chunk, encoding, (err, data) => {
      if (err) return cb(err);
      if (data != null) this.push(data);
      cb();
    });
  }

  _flush(cb) { cb(); }

  end(chunk, encoding, cb) {
    if (typeof chunk === "function") { cb = chunk; chunk = null; }
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (chunk != null) this.write(chunk, encoding);
    this._flush((err, data) => {
      if (data != null) this.push(data);
      this.push(null);
      this._writableState.ended = true;
      this._writableState.finished = true;
      this.emit("finish");
      if (cb) cb(err);
    });
    return this;
  }
}

class PassThrough extends Transform {
  _transform(chunk, _encoding, cb) { cb(null, chunk); }
}

function pipeline(...streams) {
  let cb;
  if (typeof streams[streams.length - 1] === "function") cb = streams.pop();
  let error = null;
  for (let i = 0; i < streams.length - 1; i++) streams[i].pipe(streams[i + 1]);
  const last = streams[streams.length - 1];
  if (cb) {
    last.on("finish", () => !error && cb(null));
    for (const s of streams) s.on("error", (err) => { if (!error) { error = err; cb(err); } });
  }
  return last;
}

function finished(stream, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = {}; }
  const cleanup = () => {
    stream.removeListener("finish", onFinish);
    stream.removeListener("error", onErr);
    stream.removeListener("end", onEnd);
    stream.removeListener("close", onClose);
  };
  const onFinish = () => { cleanup(); cb(null); };
  const onErr = (err) => { cleanup(); cb(err); };
  const onEnd = () => { cleanup(); cb(null); };
  const onClose = () => { cleanup(); cb(null); };
  stream.on("finish", onFinish);
  stream.on("error", onErr);
  stream.on("end", onEnd);
  stream.on("close", onClose);
  return cleanup;
}

const streamModule = {
  Stream, Readable, Writable, Duplex, Transform, PassThrough,
  pipeline, finished,
};

export default streamModule;
export { Stream, Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished };
