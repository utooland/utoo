const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const result = require("./output/main.js");
assert.equal(result.contains, true);
assert.equal(result.missing, false);
assert.equal(result.fragment, "fallback");
assert.match(result.resolved, /\/string\/#\/contains\/index\.js/);
assert.match(result.fallbackResolved, /\/plain\.js/);

const output = fs
  .readdirSync(path.join(__dirname, "output"))
  .filter((file) => file.endsWith(".js"))
  .map((file) => fs.readFileSync(path.join(__dirname, "output", file), "utf8"))
  .join("\n");
assert.ok(!output.includes("UNEXPECTED_HASH_BARREL"));
