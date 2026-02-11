// Minimal module module stub for utoo-runtime

class Module {
  constructor(id, parent) {
    this.id = id || "";
    this.path = "";
    this.exports = {};
    this.filename = null;
    this.loaded = false;
    this.children = [];
    this.paths = [];
    this.parent = parent || null;
  }

  require(id) {
    return globalThis.require(id);
  }
}

Module._cache = {};
Module._pathCache = {};
Module._extensions = {
  ".js": function() {},
  ".json": function() {},
  ".node": function() {},
  ".ts": function() {},
  ".cjs": function() {},
};
Module._resolveFilename = function(request) {
  return request;
};
Module.builtinModules = [
  "assert", "async_hooks", "buffer", "child_process", "cluster",
  "console", "constants", "crypto", "dgram", "diagnostics_channel",
  "dns", "domain", "events", "fs", "http", "https",
  "module", "net", "os", "path", "perf_hooks", "querystring",
  "readline", "stream", "string_decoder", "timers", "tls", "tty",
  "url", "util", "v8", "worker_threads", "zlib",
];
Module.createRequire = function(filename) {
  return globalThis.require;
};
Module.isBuiltin = function(moduleName) {
  const name = moduleName.startsWith("node:") ? moduleName.slice(5) : moduleName;
  return Module.builtinModules.includes(name);
};
Module.wrap = function(script) {
  return "(function(exports,require,module,__filename,__dirname){" + script + "\n});";
};

// Node.js compat: require('module') returns the Module class itself
Module.Module = Module;
Module.default = Module;

export default Module;
export const createRequire = Module.createRequire;
export const builtinModules = Module.builtinModules;
export const isBuiltin = Module.isBuiltin;
