const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const output = path.join(__dirname, "output");
for (const filename of fs.readdirSync(output, { recursive: true })) {
  const file = path.join(output, filename);
  if (!fs.statSync(file).isFile()) continue;
  const content = fs.readFileSync(file, "utf8");
  assert.ok(!content.includes("SERVER_IMPLEMENTATION_MUST_NOT_REACH_BROWSER"), filename);
  assert.ok(!content.includes("missing-server-implementation"), filename);
}

const context = {
  console,
  URL,
  document: { currentScript: { src: "https://example.test/main.js" } },
};
context.self = context;
vm.runInNewContext(fs.readFileSync(path.join(output, "main.js"), "utf8"), context);

async function main() {
  const api = context.ReferenceDemo;
  assert.equal(api.aliasesMatch, true);
  assert.equal(api.creations, 1);
  assert.equal(api.calls.length, 0);
  assert.equal(await api.instance.greet({ name: "Ada" }), "Hello Ada");
  assert.equal(api.calls.length, 1);
  assert.equal(api.calls[0].operation, "greet");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
