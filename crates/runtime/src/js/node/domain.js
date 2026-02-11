// Minimal domain module stub (deprecated in Node.js)
import EventEmitter from "ext:utoo_rt_ext/node/events";

class Domain extends EventEmitter {
  constructor() {
    super();
    this.members = [];
  }
  add(emitter) { this.members.push(emitter); }
  remove(emitter) {
    const i = this.members.indexOf(emitter);
    if (i >= 0) this.members.splice(i, 1);
  }
  bind(fn) { return fn; }
  intercept(fn) { return fn; }
  run(fn) { return fn(); }
  dispose() { this.removeAllListeners(); }
  enter() {}
  exit() {}
}

function create() { return new Domain(); }

const domain = { Domain, create };
domain.default = domain;

export default domain;
export { Domain, create };
