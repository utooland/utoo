const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const result = spawnSync(
  process.execPath,
  [
    "-e",
    `
      const assert = require("node:assert/strict");
      const result = require("./output/main.js");
      const { state } = require("./input/fc.js");

      assert.equal(result.sameInstance, true);
      assert.equal(result.requestInitialized, true);
      assert.equal(state.initialized, true);
    `,
  ],
  {
    cwd: __dirname,
    env: { ...process.env, NODE_PATH: path.join(__dirname, "input") },
    encoding: "utf8",
  },
);

assert.ifError(result.error);
assert.equal(result.status, 0, result.stderr || result.stdout);
