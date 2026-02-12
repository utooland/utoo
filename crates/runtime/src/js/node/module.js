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
  const filepath = typeof filename === "string" ? filename :
    (filename && filename.pathname) ? filename.pathname : String(filename);
  function createdRequire(specifier) {
    const ops = Deno.core.ops;
    const resolved = ops.op_cjs_resolve(specifier, filepath);
    if (resolved.startsWith("node:")) {
      const builtins = globalThis.__cjs_builtins;
      if (builtins) {
        if (builtins.has(resolved)) return builtins.get(resolved);
        const name = resolved.slice(5);
        if (builtins.has(name)) return builtins.get(name);
      }
      throw new Error("Cannot find built-in module '" + resolved + "'");
    }
    if (globalThis.__cjs_cache && globalThis.__cjs_cache.has(resolved)) {
      return globalThis.__cjs_cache.get(resolved).exports;
    }
    // Fallback to global require
    return globalThis.require(specifier);
  }
  createdRequire.resolve = function(specifier) {
    const ops = Deno.core.ops;
    return ops.op_cjs_resolve(specifier, filepath);
  };
  createdRequire.cache = {};
  createdRequire.main = undefined;
  createdRequire.extensions = Module._extensions;
  return createdRequire;
};
Module.createRequireFromPath = Module.createRequire;
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
