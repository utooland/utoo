"use strict";

const fs = require("node:fs");
const path = require("node:path");
const childProcess = require("node:child_process");
const { syncBuiltinESMExports } = require("node:module");
const tag = "[DEBUG-win-release-20261116]";
const preload = path.resolve(__filename).replace(/\\/g, "/");
const requireOption = `--require=${JSON.stringify(preload)}`;
const hexStatus = (code) =>
  typeof code === "number" ? `0x${(code >>> 0).toString(16).padStart(8, "0")}` : null;
function log(event, details = {}) {
  try {
    fs.writeSync(2, `${JSON.stringify({ tag, event, pid: process.pid, ...details })}\n`);
  } catch {}
}
const smallArgs = (args) => (args || []).slice(0, 8).map((arg) => String(arg).slice(0, 240));
log("process.start", {
  ppid: process.ppid,
  execPath: process.execPath,
  argv: smallArgs(process.argv),
  nodeOptions: process.env.NODE_OPTIONS,
});
process.on("exit", (code) => log("process.exit", { code, ntstatus: hexStatus(code) }));
process.on("uncaughtExceptionMonitor", (error, origin) => {
  try {
    log("process.uncaughtExceptionMonitor", { origin, message: error?.message, stack: error?.stack });
  } catch {}
});

const originalSpawn = childProcess.spawn;
childProcess.spawn = function (...args) {
  const index = Array.isArray(args[1]) || args[1] == null ? 2 : 1;
  const options = args[index];
  if (options != null && (typeof options !== "object" || Array.isArray(options))) {
    return Reflect.apply(originalSpawn, this, args);
  }
  const env = { ...(options?.env || process.env) };
  const nodeOptions = String(env.NODE_OPTIONS || "");
  env.NODE_OPTIONS = nodeOptions.includes(requireOption)
    ? nodeOptions
    : `${nodeOptions} ${requireOption}`.trim();
  args[index] = { ...options, env };
  return Reflect.apply(originalSpawn, this, args);
};

const originalEmit = childProcess.ChildProcess.prototype.emit;
childProcess.ChildProcess.prototype.emit = function (event, ...args) {
  try {
    if (["spawn", "exit", "close", "error"].includes(event)) {
      const code = event === "error" ? args[0]?.code : event === "spawn" ? null : args[0];
      log(`child.${event}`, {
        childPid: this.pid ?? null,
        spawnfile: this.spawnfile,
        args: smallArgs(this.spawnargs),
        code,
        signal: event === "exit" || event === "close" ? args[1] : null,
        ntstatus: hexStatus(code),
        message: event === "error" ? args[0]?.message : undefined,
      });
    }
  } catch {}
  return Reflect.apply(originalEmit, this, [event, ...args]);
};

const originalDlopen = process.dlopen;
process.dlopen = function (module, filename, ...args) {
  const observe = typeof filename === "string" && path.basename(filename) === "pack.win32-x64-msvc.node";
  if (observe) log("dlopen.before", { filename });
  const result = Reflect.apply(originalDlopen, this, [module, filename, ...args]);
  if (observe) log("dlopen.success", { filename });
  return result;
};
syncBuiltinESMExports();
