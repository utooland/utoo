// Override only Pack's binding, including Node loader workers.
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const { threadId } = require("node:worker_threads");

const selected = process.env.PROFILE_NATIVE_LIBRARY;
if (selected) {
  const native = fs.realpathSync(selected);
  const originalDlopen = process.dlopen;
  process.dlopen = function (module, filename, flags) {
    const result = originalDlopen.apply(this, arguments);
    if (process.env.PROFILE_NATIVE_LOAD_LOG && fs.realpathSync(filename) === native) {
      fs.appendFileSync(
        process.env.PROFILE_NATIVE_LOAD_LOG,
        `${JSON.stringify({
          pid: process.pid,
          threadId,
          native,
          sha256: process.env.PROFILE_NATIVE_SHA256,
          event: "dlopen-success",
        })}\n`,
      );
    }
    return result;
  };

  const originalLoad = Module._load;
  Module._load = function (request, parent, isMain) {
    if (/(?:^|\/)binding(?:\.js)?$/.test(request)) {
      const filename = Module._resolveFilename(request, parent, isMain);
      if (/[/\\]@utoo[/\\]pack[/\\](?:cjs|esm)[/\\]binding\.js$/.test(filename)) {
        const previous = process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
        process.env.NAPI_RS_NATIVE_LIBRARY_PATH = native;
        try {
          return originalLoad.apply(this, arguments);
        } finally {
          if (previous === undefined) delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
          else process.env.NAPI_RS_NATIVE_LIBRARY_PATH = previous;
        }
      }
    }
    return originalLoad.apply(this, arguments);
  };
}
