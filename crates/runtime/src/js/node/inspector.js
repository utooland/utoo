// Minimal inspector module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

class Session extends EventEmitter {
  constructor() { super(); }
  connect() {}
  connectToMainThread() {}
  disconnect() {}
  post(method, params, cb) {
    if (typeof params === "function") { cb = params; params = {}; }
    if (cb) cb(new Error("Inspector is not available in utoo-runtime"));
  }
}

function open() {}
function close() {}
function url() { return undefined; }
function waitForDebugger() {}

const inspector = { Session, open, close, url, waitForDebugger };
inspector.default = inspector;

export default inspector;
export { Session, open, close, url, waitForDebugger };
