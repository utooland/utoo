// node:repl stub for utoo-runtime
// Provides the minimal surface so require('repl') doesn't crash.

function REPLServer() {
  throw new Error("REPL is not supported in utoo-runtime");
}

export function start() {
  throw new Error("REPL is not supported in utoo-runtime");
}

export { REPLServer };

export default {
  start,
  REPLServer,
};
