const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const serverOutput = path.join(__dirname, "output/server");
const stats = JSON.parse(
  fs.readFileSync(path.join(serverOutput, "stats.json"), "utf8"),
);
assert.deepEqual(Object.keys(stats.entrypoints).sort(), [
  "detail-server",
  "index-server",
  "server",
]);

const cssAssets = stats.assets
  .map((asset) => asset.name)
  .filter((asset) => asset.endsWith(".css"));
assert.equal(cssAssets.length, 4);
assert.equal(new Set(cssAssets).size, 4);

const styles = cssAssets.map((asset) => {
  const content = fs.readFileSync(path.join(serverOutput, asset), "utf8");
  const comments = [...content.matchAll(/[#@]\s*sourceMappingURL=([^\s*]+)/g)];
  assert.equal(comments.length, 1, asset);
  assert.equal(
    decodeURIComponent(comments[0][1]),
    `${path.basename(asset)}.map`,
  );
  const map = JSON.parse(
    fs.readFileSync(path.join(serverOutput, `${asset}.map`), "utf8"),
  );
  assert.equal(map.version, 3, asset);
  return content;
});

for (const selector of [
  ".all-servers",
  ".primary-servers",
  ".index-page",
  ".detail-page",
]) {
  assert.equal(
    styles.filter((content) => content.includes(selector)).length,
    1,
    selector,
  );
}

// Shared styles must remain reachable from every entry that imports them.
const allStyles = cssAssets.find((_, index) =>
  styles[index].includes(".all-servers"),
);
const primaryStyles = cssAssets.find((_, index) =>
  styles[index].includes(".primary-servers"),
);
for (const name of ["server", "index-server", "detail-server"]) {
  const assets = stats.entrypoints[name].assets.map((asset) => asset.name);
  assert.ok(assets.includes(allStyles), name);
  assert.equal(assets.includes(primaryStyles), name !== "detail-server", name);
}
