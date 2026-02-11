// Minimal child_process module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

class ChildProcess extends EventEmitter {
  constructor() {
    super();
    this.stdin = null;
    this.stdout = null;
    this.stderr = null;
    this.pid = 0;
    this.connected = false;
    this.killed = false;
    this.exitCode = null;
    this.signalCode = null;
  }
  kill() { this.killed = true; return true; }
  disconnect() { this.connected = false; }
  send() { return false; }
  ref() { return this; }
  unref() { return this; }
}

function exec(cmd, opts, cb) {
  if (typeof opts === "function") { cb = opts; }
  const err = new Error("child_process.exec() is not supported in utoo-runtime");
  if (cb) cb(err, "", "");
  else throw err;
}

function execSync() {
  throw new Error("child_process.execSync() is not supported in utoo-runtime");
}

function execFile(file, args, opts, cb) {
  if (typeof args === "function") { cb = args; }
  else if (typeof opts === "function") { cb = opts; }
  const err = new Error("child_process.execFile() is not supported in utoo-runtime");
  if (cb) cb(err, "", "");
  else throw err;
}

function execFileSync() {
  throw new Error("child_process.execFileSync() is not supported in utoo-runtime");
}

function spawn() {
  throw new Error("child_process.spawn() is not supported in utoo-runtime");
}

function spawnSync() {
  throw new Error("child_process.spawnSync() is not supported in utoo-runtime");
}

function fork() {
  throw new Error("child_process.fork() is not supported in utoo-runtime");
}

const child_process = {
  ChildProcess,
  exec,
  execSync,
  execFile,
  execFileSync,
  spawn,
  spawnSync,
  fork,
};
child_process.default = child_process;

export default child_process;
export { ChildProcess, exec, execSync, execFile, execFileSync, spawn, spawnSync, fork };
