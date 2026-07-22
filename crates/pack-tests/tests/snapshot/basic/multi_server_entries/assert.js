const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const serverOutput = path.join(__dirname, "output/server");
const stats = JSON.parse(fs.readFileSync(path.join(serverOutput, "stats.json"), "utf8"));
const expectedEntrypoints = ["detail-server", "index-server", "server"];

assert.deepEqual(Object.keys(stats.entrypoints).sort(), expectedEntrypoints);
const entryAssets = {};
for (const name of expectedEntrypoints) {
  const assets = stats.entrypoints[name].assets
    .map((asset) => asset.name)
    .filter((asset) => asset.endsWith(".js"));
  assert.equal(assets.length, 2);
  entryAssets[name] = assets;
  const entryAsset = assets.find((asset) =>
    new RegExp(`^\\./${name}\\.[a-f0-9]{8}\\.js$`).test(asset),
  );
  assert.ok(entryAsset, `missing emitted bundle for ${name}`);
  const entryPath = path.join(serverOutput, entryAsset);
  assert.ok(fs.existsSync(entryPath));
}

const sharedAssets = entryAssets[expectedEntrypoints[0]].filter((asset) =>
  expectedEntrypoints.every((name) => entryAssets[name].includes(asset)),
);
assert.equal(sharedAssets.length, 1);
assert.match(sharedAssets[0], /^(?:\.\/)?server-shared\.[a-f0-9]{8}\.js$/);
const sharedPath = path.join(serverOutput, sharedAssets[0]);
assert.ok(fs.existsSync(sharedPath));
assert.match(fs.readFileSync(sharedPath, "utf8"), /shared dependency/);

for (const name of expectedEntrypoints) {
  const entryAsset = entryAssets[name].find((asset) => asset !== sharedAssets[0]);
  assert.doesNotMatch(fs.readFileSync(path.join(serverOutput, entryAsset), "utf8"), /shared dependency/);
  require(path.join(serverOutput, entryAsset));
}
