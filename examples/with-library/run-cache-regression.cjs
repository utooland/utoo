const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { build } = require("@utoo/pack");

const root = path.resolve(__dirname, "../..");
const outputRoot = path.join(__dirname, ".turbopack", "require-cache-compat");
const hasOwn = (object, key) =>
  Object.prototype.hasOwnProperty.call(object, key);

function verify(code, moduleIds, hasProxy) {
  const context = { console, URL };
  context.self = context;
  if (!hasProxy) {
    context.Proxy = undefined;
    context.globalThis = undefined;
  }
  vm.runInNewContext(code, context, {
    filename: `require-cache-${moduleIds}-${hasProxy ? "proxy" : "legacy"}.js`,
  });

  const api = context.CacheLibrary;
  const cache = api.getCache();
  assert.equal(api.getCache(), cache, "require.cache has stable identity");
  const counterId = api.counterId();
  const futureId = api.futureId();
  assert.equal(typeof counterId, moduleIds === "named" ? "string" : "number");
  assert.equal(hasOwn(cache, counterId), false);
  assert.equal(hasOwn(cache, futureId), false);

  const first = api.readCounter();
  assert.equal(first.runs, 1);
  assert.equal(cache[counterId].exports, first);
  assert(Object.keys(cache).includes(String(counterId)));
  assert.equal(api.readCounter(), first, "reads use the live cached module");

  const replacement = { replaced: true };
  cache[counterId] = { ...cache[counterId], exports: replacement };
  assert.equal(api.readCounter(), replacement, "replacement changes require()");
  delete cache[counterId];
  assert.equal(hasOwn(cache, counterId), false);
  assert(!Object.keys(cache).includes(String(counterId)));
  const reloaded = api.readCounter();
  assert.equal(reloaded.runs, 2, "deleted modules are evaluated again");
  assert.equal(cache[counterId].exports, reloaded);

  const inserted = { inserted: true };
  cache[futureId] = {
    id: futureId,
    exports: inserted,
    error: undefined,
    parents: [],
    children: [],
  };
  assert.equal(api.readFuture(), inserted, "insertion skips the module factory");
  assert.equal(context.cacheFutureRuns, undefined);
  assert(Object.keys(cache).includes(String(futureId)));
  delete cache[futureId];
  assert.equal(api.readFuture().runs, 1);
  assert.equal(context.cacheFutureRuns, 1);

  const arbitraryKeys = ["1", "01", "1e3", "__proto__", "toString"];
  const entries = arbitraryKeys.map((key) => ({ exports: { key } }));
  arbitraryKeys.forEach((key, index) => {
    cache[key] = entries[index];
  });
  arbitraryKeys.forEach((key, index) => {
    assert.equal(cache[key], entries[index]);
    assert.equal(hasOwn(cache, key), true);
    assert(Object.keys(cache).includes(key));
  });
  assert.equal(api.readCounter(), reloaded, "arbitrary keys do not alias modules");
  arbitraryKeys.forEach((key) => {
    delete cache[key];
    assert.equal(hasOwn(cache, key), false);
  });

  // Non-canonical numeric spellings must stay distinct from actual numeric ids.
  if (moduleIds !== "named") {
    const alias = `0${counterId}`;
    cache[alias] = { exports: { wrong: true } };
    assert.equal(api.readCounter(), reloaded);
    delete cache[alias];
  }
  console.log(
    `require.cache ${moduleIds}/${hasProxy ? "Proxy" : "Proxy unavailable"}: passed`,
  );
}

async function main() {
  fs.rmSync(outputRoot, { recursive: true, force: true });
  for (const moduleIds of ["named", "deterministic"]) {
    const output = path.join(outputRoot, moduleIds);
    await build(
      {
        config: {
          mode: "production",
          target: "Chrome 41",
          persistentCaching: false,
          entry: [
            {
              name: "main",
              import: "./cache-compat/index.js",
              library: { name: "CacheLibrary" },
            },
          ],
          output: { path: output, clean: true },
          sourceMaps: false,
          optimization: {
            minify: false,
            concatenateModules: false,
            moduleIds,
          },
        },
      },
      __dirname,
      root,
    );
    const code = fs.readFileSync(path.join(output, "main.js"), "utf8");
    verify(code, moduleIds, true);
    verify(code, moduleIds, false);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
