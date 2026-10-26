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
const context = { window: {}, document };
const bundle = fs.readFileSync(filename, "utf8");
assert.match(bundle, /\/\/ Dummy runtime/);
vm.runInNewContext(
  bundle.replace("// Dummy runtime", "globalThis.chunks = __UTOOPACK__;"),
  context,
  { filename },
);

// Execute the emitted factories with only the primitives this fixture uses.
const factories = new Map();
const entries = [];
for (const chunk of context.chunks) {
  entries.push(...(chunk[1]?.runtimeModuleIds ?? []));
  for (let index = 1; index < chunk.length; index += 2) {
    if (typeof chunk[index + 1] === "function") {
      factories.set(chunk[index], chunk[index + 1]);
    }
  }
}
const modules = new Map();
function load(id) {
  if (modules.has(id)) return modules.get(id);
  const factory = factories.get(id);
  assert.ok(factory, `missing module ${id}`);
  const namespace = {};
  const module = { exports: {} };
  modules.set(id, namespace);
  factory(
    {
      i: load,
      v(value) {
        namespace.default = value;
      },
      s(bindings) {
        for (let index = 0; index < bindings.length; index += 3) {
          assert.equal(bindings[index + 1], 0, "expected a value export");
          namespace[bindings[index]] = bindings[index + 2];
        }
      },
    },
    module,
    module.exports,
  );
  namespace.default ??= module.exports;
  return namespace;
}
assert.equal(entries.length, 1);
assert.equal(load(entries[0]).loaded, true);

assert.equal(injectedStyles.length, 1, "the imported stylesheet must auto-inject");
const css = injectedStyles.map((style) => style.textContent).join("\n");
assert.match(css, /\.commonjs-theme\s*\{\s*color:\s*red/);
