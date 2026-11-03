const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

function normalizedRealPath(filename) {
  return fs.realpathSync(filename).replaceAll("\\", "/");
}

module.exports = {
  plugins: [
    {
      postcssPlugin: "assert-postcss-input-paths",
      Once(_root, { result }) {
        const expected = normalizedRealPath(
          path.join(__dirname, "input", "style.css"),
        );
        for (const field of ["from", "to"]) {
          const filename = result.opts[field];
          assert.equal(typeof filename, "string", `PostCSS ${field} is missing`);
          assert.ok(
            path.isAbsolute(filename),
            `PostCSS ${field} must be absolute: ${filename}`,
          );
          assert.equal(normalizedRealPath(filename), expected);
        }
      },
    },
    [
      "postcss-plugin-px2rem",
      {
        rootValue: 16,
        unitPrecision: 5,
        replace: true,
        mediaQuery: false,
        minPixelValue: 0
      }
    ]
  ]
};
