import EventEmitter from "ext:utoo_rt_ext/node/events";
import { Buffer } from "ext:utoo_rt_ext/node/buffer";

const ops = Deno.core.ops;
const asyncOps = Deno.core.ops;

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
    this._rid = null;
    this.channel = null;
  }
  kill(signal) {
    if (this._rid != null) {
      try { ops.op_spawn_kill(this._rid, typeof signal === "number" ? signal : 15); } catch(e) {}
    }
    this.killed = true;
    return true;
  }
  disconnect() { this.connected = false; }
  send(msg, handle, options, cb) {
    if (typeof cb === "function") cb(new Error("IPC not supported"));
    return false;
  }
  ref() { return this; }
  unref() { return this; }
}

function _buildEnvPairs(envObj) {
  if (!envObj) return [];
  // Ensure all keys and values are strings (serde expects Vec<(String, String)>)
  return Object.entries(envObj).filter(([k, v]) => v != null).map(([k, v]) => [String(k), String(v)]);
}

function spawn(cmd, args, options) {
  if (!Array.isArray(args)) { options = args; args = []; }
  options = options || {};

  const envPairs = _buildEnvPairs(options.env || process.env);
  const cwd = options.cwd || "";

  // Handle shell option
  let actualCmd = cmd;
  let actualArgs = args || [];
  if (options.shell) {
    const shellBin = typeof options.shell === "string" ? options.shell : "/bin/sh";
    const fullCmd = [cmd, ...(args || [])].join(" ");
    actualCmd = shellBin;
    actualArgs = ["-c", fullCmd];
  }

  // stdio config
  let stdio = ["pipe", "pipe", "pipe"];
  if (options.stdio) {
    if (typeof options.stdio === "string") {
      stdio = [options.stdio, options.stdio, options.stdio];
    } else if (Array.isArray(options.stdio)) {
      stdio = options.stdio.slice(0, 3).map(s => s || "pipe");
      while (stdio.length < 3) stdio.push("pipe");
    }
  }
  // Map to strings
  stdio = stdio.map(s => {
    if (s === "ignore") return "null";
    if (s === null || s === undefined) return "pipe";
    return String(s);
  });

  const cp = new ChildProcess();

  try {
    const result = ops.op_spawn(actualCmd, actualArgs, stdio, cwd, envPairs);
    cp._rid = result.rid;
    cp.pid = result.pid;

    if (result.stdin_rid != null) {
      cp.stdin = new StdioWritable(result.stdin_rid);
    }
    if (result.stdout_rid != null) {
      cp.stdout = new StdioReadable(result.stdout_rid);
    }
    if (result.stderr_rid != null) {
      cp.stderr = new StdioReadable(result.stderr_rid);
    }

    // Wait for exit asynchronously
    asyncOps.op_spawn_wait(result.rid).then(waitResult => {
      cp.exitCode = waitResult.code;
      cp.emit("exit", waitResult.code, null);
      cp.emit("close", waitResult.code, null);
    }).catch(e => {
      cp.emit("error", e);
    });
  } catch (e) {
    process.nextTick(() => cp.emit("error", e));
  }

  return cp;
}

// Simple readable stream for child stdout/stderr
class StdioReadable extends EventEmitter {
  constructor(rid) {
    super();
    this._rid = rid;
    this.readable = true;
    this._reading = false;
    this._ended = false;
    // Start reading on next tick if there are data listeners
    const self = this;
    const origOn = this.on.bind(this);
    this.on = function(event, listener) {
      origOn(event, listener);
      if (event === "data" && !self._reading) {
        self._startReading();
      }
      return self;
    };
    this.addListener = this.on;
  }
  _startReading() {
    if (this._reading) return;
    this._reading = true;
    const self = this;
    const opName = "op_spawn_stdout_read"; // works for both stdout and stderr via different rids
    (async function readLoop() {
      while (!self._ended) {
        try {
          const data = await asyncOps.op_spawn_stdout_read(self._rid, 65536);
          if (!data || data.length === 0) {
            self._ended = true;
            self.emit("end");
            break;
          }
          self.emit("data", Buffer.from(data));
        } catch (e) {
          self._ended = true;
          if (!e.message.includes("closed")) {
            self.emit("error", e);
          }
          self.emit("end");
          break;
        }
      }
    })();
  }
  pipe(dest) {
    this.on("data", chunk => dest.write(chunk));
    this.on("end", () => { if (dest !== process.stdout && dest !== process.stderr) dest.end(); });
    return dest;
  }
  setEncoding(enc) { this._encoding = enc; return this; }
  destroy() { this._ended = true; }
}

// Simple writable for child stdin
class StdioWritable extends EventEmitter {
  constructor(rid) {
    super();
    this._rid = rid;
    this.writable = true;
  }
  write(data, encoding, cb) {
    if (typeof encoding === "function") { cb = encoding; encoding = undefined; }
    const buf = typeof data === "string" ? Buffer.from(data, encoding || "utf8") : data;
    asyncOps.op_spawn_stdin_write(this._rid, buf).then(() => {
      if (cb) cb(null);
    }).catch(e => {
      if (cb) cb(e);
    });
    return true;
  }
  end(data, encoding, cb) {
    if (data) {
      this.write(data, encoding, () => {
        try { ops.op_spawn_stdin_close(this._rid); } catch(e) {}
        if (cb) cb();
      });
    } else {
      try { ops.op_spawn_stdin_close(this._rid); } catch(e) {}
      if (cb) cb();
    }
  }
  destroy() {
    try { ops.op_spawn_stdin_close(this._rid); } catch(e) {}
  }
}

function execSync(cmd, options) {
  options = options || {};
  const envPairs = _buildEnvPairs(options.env || process.env);
  const cwd = options.cwd || "";
  const shell = typeof options.shell === "string" ? options.shell : "/bin/sh";

  const result = ops.op_exec_sync(cmd, [], cwd, envPairs, shell);

  if (result.status !== 0 && !options.ignoreExitCode) {
    const err = new Error("Command failed: " + cmd);
    err.status = result.status;
    err.stdout = Buffer.from(result.stdout);
    err.stderr = Buffer.from(result.stderr);
    err.output = [null, err.stdout, err.stderr];
    throw err;
  }

  if (options.encoding === "buffer" || !options.encoding) {
    return Buffer.from(result.stdout);
  }
  return Buffer.from(result.stdout).toString(options.encoding || "utf8");
}

function exec(cmd, options, cb) {
  if (typeof options === "function") { cb = options; options = {}; }
  options = options || {};

  const cp = spawn("/bin/sh", ["-c", cmd], {
    ...options,
    stdio: ["pipe", "pipe", "pipe"],
  });

  let stdout = "";
  let stderr = "";

  if (cp.stdout) cp.stdout.on("data", d => { stdout += d.toString(); });
  if (cp.stderr) cp.stderr.on("data", d => { stderr += d.toString(); });

  cp.on("error", e => { if (cb) cb(e, stdout, stderr); });
  cp.on("close", (code) => {
    if (cb) {
      if (code !== 0) {
        const err = new Error("Command failed: " + cmd);
        err.code = code;
        cb(err, stdout, stderr);
      } else {
        cb(null, stdout, stderr);
      }
    }
  });

  return cp;
}

function execFile(file, args, options, cb) {
  if (typeof args === "function") { cb = args; args = []; options = {}; }
  else if (typeof options === "function") { cb = options; options = {}; }
  options = options || {};

  const cp = spawn(file, args || [], {
    ...options,
    stdio: ["pipe", "pipe", "pipe"],
  });

  let stdout = "";
  let stderr = "";

  if (cp.stdout) cp.stdout.on("data", d => { stdout += d.toString(); });
  if (cp.stderr) cp.stderr.on("data", d => { stderr += d.toString(); });

  cp.on("error", e => { if (cb) cb(e, stdout, stderr); });
  cp.on("close", (code) => {
    if (cb) {
      if (code !== 0) {
        const err = new Error("Command failed: " + file);
        err.code = code;
        cb(err, stdout, stderr);
      } else {
        cb(null, stdout, stderr);
      }
    }
  });

  return cp;
}

function execFileSync(file, args, options) {
  options = options || {};
  const envPairs = _buildEnvPairs(options.env || process.env);
  const cwd = options.cwd || "";

  const result = ops.op_exec_sync(file, args || [], cwd, envPairs, "");

  if (result.status !== 0) {
    const err = new Error("Command failed: " + file);
    err.status = result.status;
    err.stdout = Buffer.from(result.stdout);
    err.stderr = Buffer.from(result.stderr);
    throw err;
  }

  if (options.encoding === "buffer" || !options.encoding) {
    return Buffer.from(result.stdout);
  }
  return Buffer.from(result.stdout).toString(options.encoding || "utf8");
}

function spawnSync(cmd, args, options) {
  if (!Array.isArray(args)) { options = args; args = []; }
  options = options || {};
  const envPairs = _buildEnvPairs(options.env || process.env);
  const cwd = options.cwd || "";
  const shell = options.shell ? (typeof options.shell === "string" ? options.shell : "/bin/sh") : "";

  let actualCmd = cmd;
  let actualArgs = args || [];
  if (shell) {
    actualCmd = shell;
    actualArgs = ["-c", [cmd, ...(args || [])].join(" ")];
  }

  const result = ops.op_exec_sync(actualCmd, actualArgs, cwd, envPairs, "");

  return {
    status: result.status,
    stdout: Buffer.from(result.stdout),
    stderr: Buffer.from(result.stderr),
    output: [null, Buffer.from(result.stdout), Buffer.from(result.stderr)],
    pid: 0,
    signal: null,
    error: result.status !== 0 ? new Error("Command failed") : undefined,
  };
}

function fork(modulePath, args, options) {
  if (!Array.isArray(args)) { options = args; args = []; }
  options = options || {};

  const execPath = options.execPath || process.execPath;
  const execArgv = options.execArgv || process.execArgv || [];
  const allArgs = [...execArgv, modulePath, ...(args || [])];

  // Build env pairs and stdio config (same logic as spawn)
  const envPairs = _buildEnvPairs(options.env || process.env);
  const cwd = String(options.cwd || "");

  let stdio = options.stdio || ["pipe", "inherit", "inherit"];
  if (typeof stdio === "string") {
    stdio = [stdio, stdio, stdio];
  } else if (Array.isArray(stdio)) {
    stdio = stdio.slice(0, 3).map(s => s || "pipe");
    while (stdio.length < 3) stdio.push("pipe");
  }
  stdio = stdio.map(s => {
    if (s === "ignore") return "null";
    if (s === "ipc") return "pipe"; // IPC is handled separately
    if (s === null || s === undefined) return "pipe";
    return String(s);
  });

  const cp = new ChildProcess();

  try {
    // Use op_fork_spawn which creates a socketpair for IPC
    const result = ops.op_fork_spawn(String(execPath), allArgs.map(String), stdio, cwd, envPairs);
    cp._rid = result.rid;
    cp.pid = result.pid;
    cp._ipcRid = result.ipc_rid;
    cp.connected = true;
    cp.channel = { ref() {}, unref() {} };

    if (result.stdin_rid != null) {
      cp.stdin = new StdioWritable(result.stdin_rid);
    }
    if (result.stdout_rid != null) {
      cp.stdout = new StdioReadable(result.stdout_rid);
    }
    if (result.stderr_rid != null) {
      cp.stderr = new StdioReadable(result.stderr_rid);
    }

    // Set up cp.send() for parent -> child IPC
    cp.send = function(message, handle, options2, callback) {
      if (typeof handle === "function") { callback = handle; handle = undefined; options2 = undefined; }
      if (typeof options2 === "function") { callback = options2; options2 = undefined; }
      if (!cp.connected || cp._ipcRid == null) {
        if (typeof callback === "function") callback(new Error("IPC channel closed"));
        return false;
      }
      const json = JSON.stringify(message);
      asyncOps.op_ipc_stream_write(cp._ipcRid, json).then(() => {
        if (typeof callback === "function") callback(null);
      }).catch(e => {
        if (typeof callback === "function") callback(e);
      });
      return true;
    };

    // Start IPC read loop: parent reads messages from child
    (async function ipcReadLoop() {
      while (cp.connected && cp._ipcRid != null) {
        try {
          const line = await asyncOps.op_ipc_stream_read_line(cp._ipcRid);
          if (!line) {
            // EOF
            cp.connected = false;
            cp.channel = null;
            cp.emit("disconnect");
            break;
          }
          try {
            const msg = JSON.parse(line);
            cp.emit("message", msg);
          } catch (e) {
            // Non-JSON line, ignore
          }
        } catch (e) {
          cp.connected = false;
          cp.channel = null;
          cp.emit("disconnect");
          break;
        }
      }
    })();

    // Wait for exit asynchronously
    asyncOps.op_spawn_wait(result.rid).then(waitResult => {
      cp.exitCode = waitResult.code;
      cp.connected = false;
      cp.channel = null;
      cp.emit("exit", waitResult.code, null);
      cp.emit("close", waitResult.code, null);
    }).catch(e => {
      cp.emit("error", e);
    });
  } catch (e) {
    process.nextTick(() => cp.emit("error", e));
  }

  return cp;
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
