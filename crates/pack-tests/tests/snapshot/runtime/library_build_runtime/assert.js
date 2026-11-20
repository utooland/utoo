const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const filename = path.join(__dirname, "output/main.js");
const bundle = fs.readFileSync(filename, "utf8");
const logs = [];
const console = { log: (value) => logs.push(value) };
const globalContext = { URL, console };
vm.runInNewContext(bundle, globalContext, { filename });

const commonJsContext = { URL, console, module: { exports: {} }, exports: {} };
vm.runInNewContext(bundle, commonJsContext, { filename });

const entryUrl = "file:///ROOT/input/index.js";
const dependencyUrl =
  "file:///ROOT/input/meta%20space%20%25%20%E4%B8%AD%E6%96%87.mjs";
const expectedEnv = {
  DEV: false,
  PROD: true,
  MODE: "production",
  BASE_URL: "/",
  SSR: false,
};

for (const library of [globalContext.MyLibrary, commonJsContext.module.exports]) {
  assert.equal(library.answer, 42, "the imported .mjs module must be bundled");
  assert.match(library.icon, /^\/static\/icon\.[0-9a-f]{8}\.svg$/);
  // Snapshot fixtures trim surrounding whitespace when writing text outputs.
  assert.equal(
    fs.readFileSync(path.join(__dirname, "output", library.icon), "utf8").trim(),
    fs.readFileSync(path.join(__dirname, "input/icon.svg"), "utf8").trim(),
  );
  assert.equal(library.entryUrl, entryUrl);
  assert.equal(library.readEntryUrl(), entryUrl);
  assert.equal(library.dependencyUrl, dependencyUrl);
  assert.equal(library.readDependencyUrl(), dependencyUrl);
  assert.deepEqual({ ...library.entryEnv }, expectedEnv);
  assert.deepEqual({ ...library.dependencyEnv }, expectedEnv);
  assert.equal(new URL(library.dependencyUrl).hash, "");
}

assert.deepEqual(logs, ["Hello, world!", "Hello, world!"]);
