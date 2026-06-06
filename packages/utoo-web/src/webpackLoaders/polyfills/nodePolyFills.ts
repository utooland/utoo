import * as fs from "./fsPolyfill";
import * as workerThreads from "./workerThreadsPolyfill";

const bufferPolyfilled = require("buffer");
self.Buffer = bufferPolyfilled.Buffer;
const processPolyfilled = require("process");
const originalCwd = processPolyfilled.cwd;
processPolyfilled.cwd = () => {
  // @ts-ignore
  return workerThreads.workerData?.cwd || originalCwd?.() || "/";
};
if (!processPolyfilled.versions) processPolyfilled.versions = {};
if (!processPolyfilled.versions.node)
  processPolyfilled.versions.node = "24.0.0";
self.process = processPolyfilled;
self.global = self;

const path = require("path");
const originalResolve = path.resolve;
path.resolve = (...args: string[]) => {
  // @ts-ignore
  const cwd = workerThreads.workerData?.cwd || "/";
  return originalResolve(cwd, ...args);
};

const workerThreadsWithWorkerData = {
  ...workerThreads,
  get workerData() {
    return workerThreads.workerData;
  },
  get threadId() {
    return workerThreads.workerData?.threadId || 0;
  },
};

// Used to directly inject polyfill instance into systemjs
export default {
  get assert() {
    return require("assert");
  },
  get "node:assert"() {
    return require("assert");
  },

  buffer: bufferPolyfilled,
  "node:buffer": bufferPolyfilled,

  get child_process() {
    return require("child_process");
  },
  get "node:child_process"() {
    return require("child_process");
  },

  get cluster() {
    return require("cluster");
  },
  get "node:cluster"() {
    return require("cluster");
  },

  get console() {
    return require("console");
  },
  get "node:console"() {
    return require("console");
  },

  get constants() {
    return require("constants");
  },
  get "node:constants"() {
    return require("constants");
  },

  get crypto() {
    return require("crypto");
  },
  get "node:crypto"() {
    return require("crypto");
  },

  get dgram() {
    return require("dgram");
  },
  get "node:dgram"() {
    return require("dgram");
  },

  get dns() {
    return require("dns");
  },
  get "node:dns"() {
    return require("dns");
  },

  get domain() {
    return require("domain");
  },
  get "node:domain"() {
    return require("domain");
  },

  get events() {
    return require("events");
  },
  get "node:events"() {
    return require("events");
  },

  get http() {
    return require("http");
  },
  get "node:http"() {
    return require("http");
  },

  get http2() {
    return require("http2");
  },
  get "node:http2"() {
    return require("http2");
  },

  get https() {
    return require("https");
  },
  get "node:https"() {
    return require("https");
  },

  get module() {
    return {
      createRequire: (self as any).createRequire,
    };
  },
  get "node:module"() {
    return {
      createRequire: (self as any).createRequire,
    };
  },

  get net() {
    return require("net");
  },
  get "node:net"() {
    return require("net");
  },

  get os() {
    return require("os");
  },
  get "node:os"() {
    return require("os");
  },

  get punycode() {
    return require("punycode");
  },
  get "node:punycode"() {
    return require("punycode");
  },

  get querystring() {
    return require("querystring");
  },
  get "node:querystring"() {
    return require("querystring");
  },

  get readline() {
    return require("readline");
  },
  get "node:readline"() {
    return require("readline");
  },

  get repl() {
    return require("repl");
  },
  get "node:repl"() {
    return require("repl");
  },

  get stream() {
    return require("stream");
  },
  get "node:stream"() {
    return require("stream");
  },

  get string_decoder() {
    return require("string_decoder");
  },
  get "node:string_decoder"() {
    return require("string_decoder");
  },

  get sys() {
    return require("util");
  },
  get "node:sys"() {
    return require("util");
  },

  get timers() {
    return require("timers");
  },
  get "node:timers"() {
    return require("timers");
  },

  get tls() {
    return require("tls");
  },
  get "node:tls"() {
    return require("tls");
  },

  get tty() {
    return require("tty");
  },
  get "node:tty"() {
    return require("tty");
  },

  get vm() {
    return require("vm");
  },
  get "node:vm"() {
    return require("vm");
  },

  get zlib() {
    return require("zlib");
  },
  get "node:zlib"() {
    return require("zlib");
  },

  fs,
  "node:fs": fs,

  path,
  "node:path": path,

  process: processPolyfilled,
  "node:process": processPolyfilled,

  get url() {
    return require("url");
  },
  get "node:url"() {
    return require("url");
  },

  get util() {
    return require("util");
  },
  get "node:util"() {
    return require("util");
  },

  worker_threads: workerThreadsWithWorkerData,
  "node:worker_threads": workerThreadsWithWorkerData,
};
