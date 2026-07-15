const assert = require("node:assert");
const fs = require("node:fs");

const runtime = fs.readFileSync("./output/main.js", "utf8");

assert.match(
  runtime,
  /contextPrototype\.x = externalRequire/,
  "browser runtime must include CommonJS external support",
);
assert.match(
  runtime,
  /contextPrototype\.y = externalImport/,
  "browser runtime must include ESM external support",
);
