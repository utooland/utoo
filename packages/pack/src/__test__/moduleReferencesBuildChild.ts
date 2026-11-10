import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import vm from "node:vm";
import { parse } from "acorn";
import type { ConfigComplete } from "../config/types";

const [, , projectPath, scenario] = process.argv;
assert(projectPath);
assert(
  [
    "enabled",
    "legacy",
    "legacy-tree-shaking",
    "modern-tree-shaking",
    "omitted",
    "empty",
    "symlink-file-resource",
    "symlink-directory-resource",
  ].includes(scenario),
);

const legacy = scenario.startsWith("legacy");
const treeShaking = scenario.endsWith("tree-shaking");
const application = legacy || treeShaking;
const selectedMarker = "SELECTED_IMPLEMENTATION_MUST_NOT_REACH_BROWSER";
const otherMarker = "UNSELECTED_IMPLEMENTATION_MUST_REMAIN";
const sources: Record<string, string> = {
  "selected/greeting.js": `
    globalThis.selectedEvaluations = (globalThis.selectedEvaluations || 0) + 1;
    export class Greeting {
      greet() { return "__LOADER_VALUE__:${selectedMarker}"; }
    }
    export default Greeting;
  `,
  "other/greeting.js": `
    globalThis.otherEvaluations = (globalThis.otherEvaluations || 0) + 1;
    export class Greeting {
      greet() { return "__LOADER_VALUE__:${otherMarker}"; }
    }
    export default Greeting;
  `,
  "barrel.js": `
    export { Greeting, default as DefaultGreeting } from "./selected/greeting.js";
    export { Greeting as OtherGreeting } from "./other/greeting.js";
    export const barrelValue = "BARREL_EXPORT_MUST_REMAIN";
  `,
  "live.js": `
    import { readCycleCounter, incrementFromCycle } from "./cycle.js";
    export let counter = 0;
    export function increment() { counter++; return counter; }
    export function setCounter(value) { counter = value; return readCycleCounter(); }
    export function readCounterFromCycle() { return readCycleCounter(); }
    export function incrementRoundTrip() { return incrementFromCycle(); }
  `,
  "cycle.js": `
    import { counter, increment } from "./live.js";
    export function readCycleCounter() { return counter; }
    export function incrementFromCycle() { increment(); return readCycleCounter(); }
  `,
  "live-barrel.js": `
    export {
      counter, increment, setCounter, readCounterFromCycle, incrementRoundTrip,
    } from "./live.js";
  `,
  "runtime.js": `
    export let creations = 0;
    export const calls = [];
    export function createReference(data) {
      creations++;
      return class GreetingProxy {
        greet() { calls.push(data.operation); return "proxy:" + data.operation; }
      };
    }
  `,
  "index.js": `
    import DirectDefault, { Greeting as Direct } from "./selected/greeting.js";
    import AliasDefault, { Greeting as Alias } from "selected-alias";
    import { Greeting as LinkedFile } from "./linked-greeting.js";
    import { Greeting as LinkedDirectory } from "./linked-directory/greeting.js";
    import { Greeting as Barrel, DefaultGreeting, OtherGreeting, barrelValue } from "./barrel.js";
    import QueryDefault, { Greeting as Query } from "./selected/greeting.js?variant=one";
    import { Greeting as QueryTwo } from "./selected/greeting.js?variant=two";
    import OtherDefault, { Greeting as Other } from "./other/greeting.js";
    import { Greeting as OtherAlias } from "other-alias";
    import { Greeting as OtherQuery } from "./other/greeting.js?variant=one";
    import { calls, creations } from "./runtime.js";
    import * as live from "./live-barrel.js";
    export {
      Direct, DirectDefault, Alias, AliasDefault, LinkedFile, LinkedDirectory,
      Barrel, DefaultGreeting, Query, QueryDefault, QueryTwo,
      Other, OtherDefault, OtherAlias, OtherGreeting, OtherQuery,
      barrelValue, calls, creations, live,
    };
  `,
};

if (application) {
  sources["runtime.js"] = `
    export let creations = 0;
    export const calls = [];
    export class GreetingFailure extends Error {
      constructor(message) {
        super(message);
        this.name = "GreetingFailure";
        this.code = "REFERENCE_FAILED";
      }
    }
    export function createReference(data) {
      creations++;
      return class GreetingProxy {
        greet() { calls.push(data.operation); return "proxy:" + data.operation; }
        async greetAsync(fail) {
          await Promise.resolve();
          if (fail) throw new GreetingFailure(data.operation);
          return this.greet();
        }
      };
    }
  `;
  sources["index.js"] = sources["index.js"].replace(
    "import { calls, creations }",
    "import { calls, creations, GreetingFailure }",
  );
  sources["index.js"] += `
    self.ReferenceDemo = {
      Direct, DirectDefault, Alias, AliasDefault, LinkedFile, LinkedDirectory,
      Barrel, DefaultGreeting, Query, QueryDefault, QueryTwo,
      Other, OtherDefault, OtherAlias, OtherGreeting, OtherQuery,
      barrelValue, calls, creations, GreetingFailure, live,
    };
  `;
  // Keep transform helper resolution inside the fixture's project root.
  const require = createRequire(import.meta.url);
  for (const name of ["@swc/helpers", "tslib"]) {
    fs.cpSync(
      path.dirname(require.resolve(`${name}/package.json`)),
      path.join(projectPath, "node_modules", name),
      { recursive: true },
    );
  }
}

fs.writeFileSync(path.join(projectPath, "package.json"), '{"private":true}');
for (const [filename, source] of Object.entries(sources)) {
  const target = path.join(projectPath, "src", filename);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, source);
}
const selectedPath = path.join(projectPath, "src/selected/greeting.js");
const fileLink = path.join(projectPath, "src/linked-greeting.js");
const directoryLink = path.join(projectPath, "src/linked-directory");
fs.symlinkSync(selectedPath, fileLink, "file");
fs.symlinkSync(
  path.dirname(selectedPath),
  directoryLink,
  process.platform === "win32" ? "junction" : "dir",
);
fs.writeFileSync(
  path.join(projectPath, "fixture-loader.cjs"),
  'module.exports = function(source) { return source.replaceAll("__LOADER_VALUE__", "loader-ran"); };',
);

const reference = {
  resource:
    scenario === "symlink-file-resource"
      ? fileLink
      : scenario === "symlink-directory-resource"
        ? path.join(directoryLink, "greeting.js")
        : fs.realpathSync(selectedPath),
  references: [
    {
      id: "greeting",
      factory: {
        module: path.join(projectPath, "src/runtime.js"),
        export: "createReference",
      },
      data: { operation: "greet" },
    },
  ],
  exports: [
    { name: "Greeting", reference: "greeting" },
    { name: "default", reference: "greeting" },
  ],
};
const config: ConfigComplete = {
  mode: "production",
  target: legacy ? "ios >= 8, android >= 5" : "chrome 120",
  persistentCaching: false,
  sourceMaps: true,
  entry: [
    {
      import: "src/index.js",
      name: "main",
      ...(application ? {} : { library: { name: "ReferenceDemo" } }),
    },
  ],
  output: { path: "output" },
  optimization: {
    minify: application,
    moduleIds: "named",
    ...(treeShaking ? { treeShaking: true } : {}),
  },
  resolve: {
    alias: {
      ...(application ? { "@swc/helpers": "./node_modules/@swc/helpers" } : {}),
      "selected-alias": "./src/selected/greeting.js",
      "other-alias": "./src/other/greeting.js",
    },
  },
  module: {
    rules: {
      "*.js": {
        loaders: [path.join(projectPath, "fixture-loader.cjs")],
        as: "*.js",
      },
    },
    ...(scenario === "omitted"
      ? {}
      : { references: scenario === "empty" ? [] : [reference] }),
  },
};

function outputFiles(directory: string): string[] {
  if (!fs.existsSync(directory)) return [];
  return fs
    .readdirSync(directory, { recursive: true, encoding: "utf8" })
    .map((filename) => path.join(directory, filename))
    .filter((filename) => fs.statSync(filename).isFile());
}

async function main() {
  const { moduleReferencesVersion } = await import("../core/moduleReferences");
  assert.equal(
    moduleReferencesVersion,
    1,
    "Tests require the current native binding",
  );
  const { build } = await import("../commands/build");
  const output = path.join(projectPath, "output");
  const buildFixture = () =>
    build(
      { config, tracing: false, buildId: "reference-matching" },
      projectPath,
      projectPath,
    );

  if (scenario.startsWith("symlink-")) {
    // The low-level contract requires a canonical resource. Noncanonical
    // registrations must fail closed, never silently emit the original source.
    await assert.rejects(
      buildFixture,
      /module reference resource must use its canonical path/,
    );
    assert.deepEqual(outputFiles(output), []);
    return;
  }

  await buildFixture();
  const files = outputFiles(output);
  const javascript = files.filter((filename) => filename.endsWith(".js"));
  const maps = files.filter((filename) => filename.endsWith(".map"));
  assert(
    javascript.length > 0 && maps.length > 0,
    "Audit must inspect JS and source maps",
  );
  const enabled = scenario === "enabled" || application;
  if (legacy) {
    for (const filename of javascript) {
      assert.doesNotThrow(
        () => parse(fs.readFileSync(filename, "utf8"), { ecmaVersion: 5 }),
        `${filename} contains syntax newer than ES5`,
      );
    }
  }
  for (const filename of [...javascript, ...maps]) {
    const content = fs.readFileSync(filename, "utf8");
    if (enabled)
      assert(
        !content.includes(selectedMarker),
        `${filename} leaked selected implementation`,
      );
  }
  const allOutput = [...javascript, ...maps]
    .map((filename) => fs.readFileSync(filename, "utf8"))
    .join("\n");
  assert(
    allOutput.includes(otherMarker),
    "Unselected same-name module was lost",
  );
  if (!enabled)
    assert(
      allOutput.includes(selectedMarker),
      "Disabled references removed original source",
    );

  const context = vm.createContext({
    console,
    URL,
    document: { currentScript: { src: "https://example.test/main.js" } },
  });
  context.self = context;
  if (application) {
    // Exercise the browser application runtime and its chunks rather than the
    // separate library emitter, which performs its own final target lowering.
    delete context.document;
    context.location = "https://example.test/";
    context.TURBOPACK_NEXT_CHUNK_URLS = [];
    context.importScripts = (url: string) => {
      const filename = path.basename(new URL(url, context.location).pathname);
      vm.runInContext(
        fs.readFileSync(path.join(output, filename), "utf8"),
        context,
        {
          filename,
        },
      );
    };
    const runtime = javascript.find((filename) =>
      path.basename(filename).startsWith("turbopack-"),
    );
    assert(runtime, "Application runtime was not emitted");
    // The worker loader records the script URL before evaluating each chunk,
    // including the initial runtime that starts the application.
    context.TURBOPACK_NEXT_CHUNK_URLS.push(
      new URL(path.basename(runtime), context.location).href,
    );
    vm.runInContext(fs.readFileSync(runtime, "utf8"), context, {
      filename: path.basename(runtime),
    });
    await new Promise((resolve) => setImmediate(resolve));
  } else {
    vm.runInContext(
      fs.readFileSync(path.join(output, "main.js"), "utf8"),
      context,
    );
  }
  const api = context.ReferenceDemo;
  for (const name of [
    "DirectDefault",
    "Alias",
    "AliasDefault",
    "LinkedFile",
    "LinkedDirectory",
    "Barrel",
    "DefaultGreeting",
  ]) {
    assert.equal(
      api[name],
      api.Direct,
      `${name} did not resolve to the same module`,
    );
  }
  assert.equal(
    api.QueryDefault,
    api.Query,
    "Same-query export aliases must share identity",
  );
  assert.notEqual(
    api.Query,
    api.Direct,
    "A query must retain its module identity",
  );
  assert.notEqual(
    api.QueryTwo,
    api.Query,
    "Different queries must remain distinct",
  );
  for (const name of ["OtherDefault", "OtherAlias", "OtherGreeting"]) {
    assert.equal(
      api[name],
      api.Other,
      `${name} changed unselected module identity`,
    );
  }
  assert.notEqual(
    api.Direct,
    api.Other,
    "Same filenames in different directories collided",
  );
  assert.equal(api.barrelValue, "BARREL_EXPORT_MUST_REMAIN");
  assert.equal(api.live.counter, 0);
  assert.equal(api.live.increment(), 1);
  assert.equal(
    api.live.counter,
    1,
    "Barrel namespace captured a stale binding",
  );
  assert.equal(api.live.readCounterFromCycle(), 1);
  assert.equal(api.live.incrementRoundTrip(), 2);
  assert.equal(api.live.counter, 2, "Cyclic imports lost an exported mutation");
  assert.equal(api.live.setCounter(9), 9);
  assert.equal(
    api.live.counter,
    9,
    "Module-fragment setter did not update its binding",
  );
  assert.equal(api.live.readCounterFromCycle(), 9);
  const selectedInstances = [api.Direct, api.Query, api.QueryTwo].map(
    (Constructor) => new Constructor(),
  );
  assert.equal(
    api.calls.length,
    0,
    "Constructing a reference must not call its method",
  );
  for (const instance of selectedInstances) {
    assert.equal(
      instance.greet(),
      enabled ? "proxy:greet" : `loader-ran:${selectedMarker}`,
    );
  }
  for (const Constructor of [api.Other, api.OtherQuery]) {
    assert.equal(new Constructor().greet(), `loader-ran:${otherMarker}`);
  }
  if (enabled) {
    assert.equal(
      context.selectedEvaluations,
      undefined,
      "Original module side effects ran",
    );
    assert.equal(api.calls.length, selectedInstances.length);
    assert.equal(api.creations, 3);
  } else {
    assert.equal(
      context.selectedEvaluations,
      3,
      "Original query modules must each execute once",
    );
    assert.equal(
      api.creations,
      0,
      "Disabled references invoked a proxy factory",
    );
    assert.equal(api.calls.length, 0);
  }
  assert.equal(
    context.otherEvaluations,
    2,
    "Unselected query modules must each execute once",
  );
  if (application) {
    assert.equal(await selectedInstances[0].greetAsync(false), "proxy:greet");
    await assert.rejects(
      selectedInstances[0].greetAsync(true),
      (error: unknown) => {
        assert(error instanceof api.GreetingFailure);
        const failure = error as Error & { code: string };
        assert.equal(failure.name, "GreetingFailure");
        assert.equal(failure.message, "greet");
        assert.equal(failure.code, "REFERENCE_FAILED");
        return true;
      },
    );
  }
}

await main();
