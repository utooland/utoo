const buffer = require("buffer");
self.Buffer = buffer.Buffer;
const process = require("process");
const originalCwd = process.cwd;
process.cwd = () => {
  // @ts-ignore
  return self.workerData?.cwd || originalCwd?.() || "/";
};
self.process = process;
self.global = self;

const path = require("path");
const originalResolve = path.resolve;
path.resolve = (...args: string[]) => {
  // @ts-ignore
  const cwd = self.workerData?.cwd || "/";
  return originalResolve(cwd, ...args);
};

import * as fs from "./fsPolyfill";
import * as workerThreads from "./workerThreadsPolyfill";

const workerThreadsWithLiveWorkerData = {
  ...workerThreads,
  get workerData() {
    // @ts-ignore
    return self.workerData;
  },
  get threadId() {
    // @ts-ignore
    return self.workerData?.workerId || 0;
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

  buffer,
  "node:buffer": buffer,

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

  fs,
  "node:fs": fs,
  "graceful-fs": fs,

  path,
  "node:path": path,

  process,
  "node:process": process,

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

  worker_threads: workerThreadsWithLiveWorkerData,
  "node:worker_threads": workerThreadsWithLiveWorkerData,

  get less() {
    return require("less/lib/less-node/index.js").default;
  },
  get "less-loader"() {
    return require("../../loaders/less-loader");
  },
  get postcss() {
    return require("postcss");
  },
  get tailwindcss() {
    return require("tailwindcss");
  },
  get "tailwindcss/lib/processTailwindFeatures"() {
    return require("tailwindcss/lib/processTailwindFeatures");
  },
  get "tailwindcss/lib/util/log"() {
    return require("tailwindcss/lib/util/log");
  },
  get "tailwindcss/resolveConfig"() {
    return require("tailwindcss/resolveConfig");
  },
  get autoprefixer() {
    return require("autoprefixer");
  },
  get "tailwindcss-animate"() {
    return require("tailwindcss-animate");
  },
};
