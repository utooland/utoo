// node:process module for utoo-runtime - re-exports the global process object.
const process = globalThis.process;

export default process;

// Live object references (env/argv) and bound methods for the common named
// imports. Primitive fields (platform/pid/version) reflect their value at load.
export const env = process.env;
export const argv = process.argv;
export const argv0 = process.argv0;
export const platform = process.platform;
export const arch = process.arch;
export const version = process.version;
export const versions = process.versions;
export const pid = process.pid;
export const cwd = process.cwd.bind(process);
export const chdir = process.chdir ? process.chdir.bind(process) : undefined;
export const nextTick = process.nextTick.bind(process);
export const hrtime = process.hrtime;
export const exit = process.exit.bind(process);
export const on = process.on.bind(process);
export const once = process.once.bind(process);
export const off = process.off ? process.off.bind(process) : undefined;
export const emit = process.emit.bind(process);
