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
  assert.equal(assets.length, name === "detail-server" ? 2 : 3);
  entryAssets[name] = assets;
  const entryAsset = assets.find((asset) =>
    new RegExp(`^(?:\\./)?entries/${name}\\.[a-f0-9]{8}\\.js$`).test(
      asset,
    ),
  );
  assert.ok(entryAsset, `missing emitted bundle for ${name}`);
  const entryPath = path.join(serverOutput, entryAsset);
  assert.ok(fs.existsSync(entryPath));
}

const allEntrySharedAssets = entryAssets[expectedEntrypoints[0]].filter((asset) =>
  expectedEntrypoints.every((name) => entryAssets[name].includes(asset)),
);
assert.equal(allEntrySharedAssets.length, 1);
assert.match(
  allEntrySharedAssets[0],
  /^(?:\.\/)?chunks\/server-shared\.[a-f0-9]{8}\.js$/,
);

const primarySharedAssets = entryAssets.server.filter(
  (asset) =>
    entryAssets["index-server"].includes(asset) &&
    !entryAssets["detail-server"].includes(asset),
);
assert.equal(primarySharedAssets.length, 1);
assert.match(
  primarySharedAssets[0],
  /^(?:\.\/)?chunks\/server-shared-0-1\.[a-f0-9]{8}\.js$/,
);

const allEntrySharedPath = path.join(serverOutput, allEntrySharedAssets[0]);
const primarySharedPath = path.join(serverOutput, primarySharedAssets[0]);
assert.ok(fs.existsSync(allEntrySharedPath));
assert.ok(fs.existsSync(primarySharedPath));
assert.match(fs.readFileSync(allEntrySharedPath, "utf8"), /shared by all entries/);
assert.match(
  fs.readFileSync(primarySharedPath, "utf8"),
  /shared by primary entries/,
);

for (const name of expectedEntrypoints) {
  const entryAsset = entryAssets[name].find((asset) =>
    /(?:^|\/)entries\//.test(asset),
  );
  const entryContent = fs.readFileSync(
    path.join(serverOutput, entryAsset),
    "utf8",
  );
  assert.doesNotMatch(entryContent, /shared by all entries/);
  assert.doesNotMatch(entryContent, /shared by primary entries/);
  require(path.join(serverOutput, entryAsset));
}
