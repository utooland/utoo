const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const outputDir = path.join(__dirname, "output");
const runtimeFile = fs
  .readdirSync(outputDir)
  .find((file) => file.startsWith("turbopack-") && file.endsWith(".js"));
assert.ok(runtimeFile, "expected a browser runtime entry");

const mainBase = "https://main.example/assets/";
const childBase = "https://child.example/assets/";
const overrideBase = "https://main.example/new-assets/";
const requestedScripts = [];
let context;

const document = {
  currentScript: null,
  querySelectorAll: () => [],
  createElement(tagName) {
    return {
      tagName,
      getAttribute(name) {
        return this[name];
      },
      addEventListener() {},
      remove() {},
    };
  },
  head: {
    appendChild(element) {
      if (element.tagName !== "script") {
        throw new Error(`unexpected element: ${element.tagName}`);
      }
      requestedScripts.push(element.src);
      const filename = path.basename(new URL(element.src).pathname);
      const source = fs.readFileSync(path.join(outputDir, filename), "utf8");
      const previousScript = document.currentScript;
      document.currentScript = element;
      try {
        vm.runInContext(source, context, { filename });
      } finally {
        document.currentScript = previousScript;
      }
    },
  },
};

context = vm.createContext({
  console,
  document,
  publicPath: mainBase,
  URL,
  setTimeout,
  clearTimeout,
});
context.self = context;
document.currentScript = {
  src: mainBase + runtimeFile,
  getAttribute(name) {
    return this[name];
  },
};
vm.runInContext(fs.readFileSync(path.join(outputDir, runtimeFile), "utf8"), context, {
  filename: runtimeFile,
});

async function waitForEntry() {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    if (context.__runtimePublicPathTest) return context.__runtimePublicPathTest;
    await new Promise(setImmediate);
  }
  throw new Error("runtime entry did not execute");
}

async function main() {
  const api = await waitForEntry();
  const initialRequestCount = requestedScripts.length;
  assert.ok(initialRequestCount > 0, "expected the entry chunk to load");

  context.publicPath = childBase;
  const first = await api.loadFirst();
  assert.equal(first.value, "first");
  assert.equal(requestedScripts.length, initialRequestCount + 1);
  assert.ok(
    requestedScripts[initialRequestCount].startsWith(mainBase),
    `main chunk used the child's public path: ${requestedScripts[initialRequestCount]}`,
  );

  api.setPublicPath(overrideBase);
  assert.equal(api.getPublicPath(), overrideBase);
  assert.equal(context.publicPath, childBase);

  const second = await api.loadSecond();
  assert.equal(second.value, "second");
  assert.equal(requestedScripts.length, initialRequestCount + 2);
  assert.ok(
    requestedScripts[initialRequestCount + 1].startsWith(overrideBase),
    `explicit __webpack_public_path__ was ignored: ${requestedScripts[initialRequestCount + 1]}`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
