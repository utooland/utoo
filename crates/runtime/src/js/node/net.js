import EventEmitter from "ext:utoo_rt_ext/node/events";
import { Duplex } from "ext:utoo_rt_ext/node/stream";

const ops = Deno.core.ops;

class Socket extends Duplex {
  constructor(opts) {
    super(opts);
    this._rid = null;
    this.remoteAddress = null;
    this.remotePort = null;
    this.remoteFamily = null;
    this.localAddress = null;
    this.localPort = null;
    this.destroyed = false;
    this.connecting = false;
    this._reading = false;
    this.allowHalfOpen = (opts && opts.allowHalfOpen) || false;

    if (opts && opts._rid !== undefined) {
      this._rid = opts._rid;
      this._afterConnect();
    }
  }

  _afterConnect() {
    this.connecting = false;
    try {
      const remote = ops.op_net_remote_addr(this._rid);
      this.remoteAddress = remote.host;
      this.remotePort = remote.port;
      this.remoteFamily = "IPv4";
    } catch {}
    try {
      const local = ops.op_net_local_addr(this._rid);
      this.localAddress = local.host;
      this.localPort = local.port;
    } catch {}
    this.emit("connect");
    this._startReading();
  }

  _startReading() {
    if (this._reading || this._rid === null) return;
    this._reading = true;
    this._readLoop();
  }

  async _readLoop() {
    while (this._reading && this._rid !== null) {
      try {
        const data = await ops.op_net_read(this._rid, 65536);
        if (!data || data.length === 0) {
          this.push(null);
          break;
        }
        this.push(Buffer.from(data));
      } catch {
        if (!this.destroyed) this.push(null);
        break;
      }
    }
  }

  connect(port, host, cb) {
    // Handle options object: connect({port, host}, cb) or connect(options, cb)
    if (typeof port === "object" && port !== null) {
      var opts = port;
      cb = typeof host === "function" ? host : cb;
      host = opts.host || opts.hostname || "127.0.0.1";
      port = opts.port;
    }
    if (typeof host === "function") { cb = host; host = "127.0.0.1"; }
    host = host || "127.0.0.1";
    port = Number(port) | 0;
    this.connecting = true;
    if (cb) this.once("connect", cb);

    (async () => {
      try {
        this._rid = await ops.op_net_connect(host, port);
        this._afterConnect();
      } catch (e) {
        this.emit("error", e);
      }
    })();
    return this;
  }

  _write(chunk, encoding, cb) {
    if (this._rid === null) return cb(new Error("Socket is closed"));
    if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);

    ops.op_net_write(this._rid, chunk).then(
      () => cb(),
      (err) => cb(err),
    );
  }

  _final(cb) {
    if (this._rid !== null) {
      ops.op_net_shutdown(this._rid).then(() => cb(), () => cb());
    } else {
      cb();
    }
  }

  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this._reading = false;
    if (this._rid !== null) {
      try { ops.op_net_close(this._rid); } catch {}
      this._rid = null;
    }
    if (err) this.emit("error", err);
    this.emit("close");
    return this;
  }

  setNoDelay() { return this; }
  setKeepAlive() { return this; }
  setTimeout(ms, cb) { if (cb) this.once("timeout", cb); return this; }
  ref() { return this; }
  unref() { return this; }
  address() {
    return { address: this.localAddress, port: this.localPort, family: "IPv4" };
  }
}

class Server extends EventEmitter {
  constructor(opts, connectionListener) {
    super();
    if (typeof opts === "function") {
      connectionListener = opts;
      opts = {};
    }
    this._opts = opts || {};
    this._rid = null;
    this._listening = false;
    this._closed = false;
    if (connectionListener) this.on("connection", connectionListener);
  }

  listen(port, host, backlog, cb) {
    // Handle the options-object form: listen({ port, host }[, cb]).
    if (typeof port === "object" && port !== null) {
      const opts = port;
      cb = typeof host === "function" ? host : cb;
      host = opts.host;
      port = opts.port;
    }
    if (typeof host === "function") { cb = host; host = "0.0.0.0"; backlog = undefined; }
    if (typeof backlog === "function") { cb = backlog; backlog = undefined; }
    host = host || "0.0.0.0";
    // Coerce the port to an i32 (env vars / config may provide it as a string).
    port = Number(port) | 0;

    // Snapshot mode: capture listen args, don't actually bind.
    // The event loop drains naturally, then V8 heap is serialized.
    if (globalThis.__utoo_snapshot_mode) {
      if (cb) this.once("listening", cb);
      if (!globalThis.__utoo_snapshot_servers) globalThis.__utoo_snapshot_servers = [];
      globalThis.__utoo_snapshot_servers.push({ server: this, port: port, host: host });
      this.emit("listening");
      return this;
    }

    if (cb) this.once("listening", cb);
    (async () => {
      try {
        this._rid = await ops.op_net_listen(host, port);
        this._listening = true;
        this.emit("listening");
        this._acceptLoop();
      } catch (e) {
        this.emit("error", e);
      }
    })();
    return this;
  }

  async _acceptLoop() {
    while (this._listening && !this._closed) {
      try {
        const connRid = await ops.op_net_accept(this._rid);
        const socket = new Socket({ _rid: connRid, allowHalfOpen: this._opts.allowHalfOpen });
        this.emit("connection", socket);
      } catch {
        if (!this._closed) break;
        break;
      }
    }
  }

  close(cb) {
    this._closed = true;
    this._listening = false;
    if (this._rid !== null) {
      try { ops.op_net_close(this._rid); } catch {}
      this._rid = null;
    }
    if (cb) this.once("close", cb);
    queueMicrotask(() => this.emit("close"));
    return this;
  }

  address() {
    if (this._rid === null) return null;
    try {
      const addr = ops.op_net_local_addr(this._rid);
      return { address: addr.host, port: addr.port, family: "IPv4" };
    } catch { return null; }
  }

  ref() { return this; }
  unref() { return this; }
  getConnections(cb) { if (cb) cb(null, 0); }
}

function createServer(opts, connectionListener) {
  return new Server(opts, connectionListener);
}

function createConnection(port, host, cb) {
  var opts = {};
  if (typeof port === "object" && port !== null) {
    opts = port;
    cb = typeof host === "function" ? host : cb;
  }
  const socket = new Socket(opts);
  return socket.connect(port, host, cb);
}

const net = {
  Server, Socket, createServer, createConnection,
  connect: createConnection,
  isIP: () => 0,
  isIPv4: (s) => /^\d+\.\d+\.\d+\.\d+$/.test(s),
  isIPv6: () => false,
};

export default net;
export { Server, Socket, createServer, createConnection };
