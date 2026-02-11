// Minimal tls module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";
import { Duplex } from "ext:utoo_rt_ext/node/stream";

class TLSSocket extends Duplex {
  constructor(socket, opts) {
    super();
    this._socket = socket;
    this.encrypted = true;
    this.authorized = false;
    this.authorizationError = null;
  }
  getPeerCertificate() { return {}; }
  getCipher() { return { name: "", version: "" }; }
  getProtocol() { return null; }
  renegotiate() {}
}

class Server extends EventEmitter {
  constructor(opts, listener) {
    super();
    if (listener) this.on("secureConnection", listener);
  }
  listen() { throw new Error("tls.Server.listen() is not supported in utoo-runtime"); }
  close(cb) { if (cb) cb(); }
  address() { return null; }
  getTicketKeys() { return Buffer.alloc(0); }
  setTicketKeys() {}
}

function createServer(opts, listener) {
  return new Server(opts, listener);
}

function connect() {
  throw new Error("tls.connect() is not supported in utoo-runtime");
}

function createSecureContext() {
  return {};
}

const DEFAULT_ECDH_CURVE = "auto";
const DEFAULT_MIN_VERSION = "TLSv1.2";
const DEFAULT_MAX_VERSION = "TLSv1.3";

const tls = {
  TLSSocket, Server, createServer, connect, createSecureContext,
  DEFAULT_ECDH_CURVE, DEFAULT_MIN_VERSION, DEFAULT_MAX_VERSION,
};
tls.default = tls;

export default tls;
export {
  TLSSocket, Server, createServer, connect, createSecureContext,
  DEFAULT_ECDH_CURVE, DEFAULT_MIN_VERSION, DEFAULT_MAX_VERSION,
};
