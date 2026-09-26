const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const output = path.join(__dirname, "output");
const context = vm.createContext({ TURBOPACK: [] });
for (const file of fs.readdirSync(output)) {
  if (file.endsWith(".js") && !file.startsWith("turbopack-")) {
    vm.runInContext(fs.readFileSync(path.join(output, file), "utf8"), context, {
      filename: file,
    });
  }
}

const factories = new Map();
for (const chunk of context.TURBOPACK) {
  let ids = [];
  for (const item of chunk.slice(1)) {
    if (typeof item === "string") {
      ids.push(item);
    } else if (typeof item === "function") {
      for (const id of ids) factories.set(id, item);
      ids = [];
    } else {
      ids = [];
    }
  }
}

function moduleId(name) {
  const suffix = `/input/${name}.js [client] (ecmascript)`;
  const id = [...factories.keys()].find((key) => key.endsWith(suffix));
  assert.ok(id, `missing emitted module ${name}`);
  return id;
}

const mainId = moduleId("main");
const listId = moduleId("List");
for (const name of ["components", "form", "outer1", "outer2"]) {
  assert.equal(factories.get(moduleId(name)), factories.get(mainId));
}
assert.notEqual(
  factories.get(listId),
  factories.get(mainId),
  "List must remain outside the merged barrel group",
);

const namespaces = new Map([...factories.keys()].map((id) => [id, {}]));
const evaluated = new Set();
function importModule(id) {
  const factory = factories.get(id);
  assert.ok(factory, `missing factory for ${id}`);
  if (!evaluated.has(factory)) {
    evaluated.add(factory);
    factory({
      i: importModule,
      s(bindings, exportId = id) {
        const namespace = namespaces.get(exportId);
        assert.ok(namespace, `missing namespace for ${exportId}`);
        for (let index = 0; index < bindings.length; ) {
          const name = bindings[index++];
          const getter = bindings[index++];
          if (getter === 0) {
            Object.defineProperty(namespace, name, {
              enumerable: true,
              value: bindings[index++],
            });
          } else {
            Object.defineProperty(namespace, name, {
              enumerable: true,
              get: getter,
            });
          }
        }
      },
    });
  }
  return namespaces.get(id);
}

importModule(mainId);
assert.deepEqual(Object.keys(context.reexportedValues).sort(), [
  "FormListContext",
  "ProFormList",
]);
assert.equal(context.reexportedValues.FormListContext, "context");
assert.equal(context.reexportedValues.ProFormList, "list");
importModule(moduleId("list-entry"));
assert.deepEqual(Array.from(context.listValues), ["context", "list"]);
