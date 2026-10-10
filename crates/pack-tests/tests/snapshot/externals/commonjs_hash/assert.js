const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const result = require("./output/main.js");
assert.equal(result.contains, true);
assert.equal(result.missing, false);
assert.equal(
  result.filename,
  require.resolve("hash-package/string/#/contains"),
);

const output = fs
  .readdirSync(path.join(__dirname, "output"))
  .filter((file) => file.endsWith(".js"))
  .map((file) => fs.readFileSync(path.join(__dirname, "output", file), "utf8"))
  .join("\n");
assert.ok(output.includes('require("hash-package/string/#/contains")'));
