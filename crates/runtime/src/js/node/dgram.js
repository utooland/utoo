// Minimal dgram module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

class Socket extends EventEmitter {
  constructor(type, listener) {
    super();
    this.type = type;
    if (listener) this.on("message", listener);
  }
  bind() { throw new Error("dgram.Socket.bind() is not supported in utoo-runtime"); }
  close(cb) { if (cb) cb(); }
  send() { throw new Error("dgram.Socket.send() is not supported in utoo-runtime"); }
  address() { return { address: "0.0.0.0", family: "IPv4", port: 0 }; }
  setBroadcast() {}
  setMulticastTTL() {}
  setMulticastLoopback() {}
  addMembership() {}
  dropMembership() {}
  ref() { return this; }
  unref() { return this; }
}

function createSocket(opts, cb) {
  const type = typeof opts === "string" ? opts : opts.type;
  return new Socket(type, cb);
}

const dgram = { createSocket, Socket };
dgram.default = dgram;

export default dgram;
export { createSocket, Socket };
