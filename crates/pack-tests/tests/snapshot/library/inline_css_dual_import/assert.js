const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const outputDir = path.join(__dirname, "output");
const outputFiles = fs.readdirSync(outputDir);
const bundle = fs.readFileSync(
  path.join(outputDir, "index.morpho.min.js"),
  "utf8",
);
const inlineStyleIds = [
  ...bundle.matchAll(
    /var update = [\s\S]*?\(\[\s*\[\s*"([^"]+\.css(?:\?modules)?)",/g,
  ),
].map((match) => match[1]);

assert.deepStrictEqual(inlineStyleIds.sort(), [
  "library/inline_css_dual_import/input/reward.less.css",
  "library/inline_css_dual_import/input/reward.less.css?modules",
]);
assert.match(bundle, /\.reward-less__[\w]+__prizeBackgroundImage \{/);
assert.match(bundle, /\\n\.prizeBackgroundImage \{/);
assert.doesNotMatch(
  outputFiles.join("\n"),
  /\.css$/,
  "inline CSS must not require a standalone stylesheet",
);
