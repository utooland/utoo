const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const outputDir = path.join(__dirname, "output");
assert.deepEqual(
  fs.readdirSync(outputDir),
  ["main.min.js"],
  "minified inline CSS must emit its JavaScript bundle without CSS or map files",
);

const injectedStyles = [];
const document = {
  querySelector(selector) {
    assert.equal(selector, "head");
    return {
      appendChild(style) {
        injectedStyles.push(style);
      },
    };
  },
  createElement(tagName) {
    assert.equal(tagName, "style");
    return {
      textContent: "",
      appendChild(node) {
        this.textContent += node.textContent;
      },
      removeAttribute(name) {
        assert.equal(name, "media");
      },
    };
  },
  createTextNode(textContent) {
    return { textContent };
  },
};

const filename = path.join(outputDir, "main.min.js");
const bundle = fs.readFileSync(filename, "utf8");
const sandbox = { URL, window: {}, document };
vm.runInNewContext(bundle, sandbox, { filename, timeout: 2000 });

assert.equal(sandbox.InlineCssMinifiedLibrary.getValue(), 42);
assert.equal(injectedStyles.length, 1, "the stylesheet must auto-inject");
assert.match(
  injectedStyles[0].textContent,
  /\.minified-inline-css\s*\{\s*color:\s*red/,
);
