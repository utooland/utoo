const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const outputDir = path.join(__dirname, "output");
assert.doesNotMatch(
  fs.readdirSync(outputDir).join("\n"),
  /\.css$/,
  "inline CSS must not require a separate stylesheet",
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
    assert.equal(tagName, "style", "styles must be injected using style tags");
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

const filename = path.join(outputDir, "main.js");
const context = { URL, window: {}, document };
vm.runInNewContext(fs.readFileSync(filename, "utf8"), context, { filename });

assert.equal(injectedStyles.length, 1, "the imported stylesheet must auto-inject");
const css = injectedStyles.map((style) => style.textContent).join("\n");
assert.match(css, /\.commonjs-theme\s*\{\s*color:\s*red/);
assert.equal(context.InlineCssCommonJsLibrary.loaded, true);
