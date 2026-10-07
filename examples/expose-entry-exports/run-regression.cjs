const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const acorn = require("acorn");

const repoRoot = path.resolve(__dirname, "../..");
const exportName = "EntryExportRegression";
const entries = ["sync-a", "sync-b", "async", "script"];

function loadBinding() {
  const addon = process.argv[2];
  if (addon) {
    assert.ok(path.isAbsolute(addon) && addon.endsWith(".node"));
    return require(addon);
  }
  const packRoot = path.dirname(
    require.resolve("@utoo/pack/package.json", { paths: [__dirname] }),
  );
  return require(path.join(packRoot, "cjs/binding.js"));
}

function walk(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((item) => {
    const file = path.join(directory, item.name);
    return item.isDirectory() ? walk(file) : [file];
  });
}

async function buildCase(binding, runDir, variant) {
  const outputDir = path.join(runDir, "dist");
  const development = variant.mode === "development";
  const config = {
    mode: variant.mode,
    target: variant.legacy ? "Chrome 41" : "Chrome 120",
    entry: entries.map((name) => ({ name, import: `./regression/${name}.js` })),
    output: {
      path: outputDir,
      publicPath: "/assets/",
      entryRootExport: exportName,
    },
    sourceMaps: false,
    externals: {
      "entry-export-script": {
        root: "EntryExportScript",
        type: "script",
        script: "https://example.test/external.js",
      },
    },
    optimization: {
      moduleIds: variant.moduleIds,
      sharedRuntime: variant.sharedRuntime,
      minify: variant.minify || false,
      concatenateModules: false,
      removeUnusedExports: false,
      removeUnusedImports: false,
      splitChunks: {
        js: {
          minChunkSize: 0,
          maxChunkCountPerGroup: 1000,
          maxMergeChunkSize: 1,
        },
      },
    },
  };
  let project;
  try {
    project = await binding.projectNew(
      {
        rootPath: process.env.UTOO_TEST_ROOT || repoRoot,
        projectPath: __dirname,
        watch: { enable: false },
        config: JSON.stringify(config),
        processEnv: [
          {
            name: "NODE_ENV",
            value: development ? "development" : "production",
          },
        ],
        dev: development,
        buildId: "entry-root-export-regression",
        tracing: false,
        packPath: path.join(repoRoot, "packages/pack/cjs"),
      },
      {
        persistentCaching: false,
        dependencyTracking: false,
        cacheDirectory: path.join(runDir, "cache"),
        turbopackGc: false,
        turbopackMemoryEviction: "off",
      },
      {
        throwTurbopackInternalError(error, options) {
          throw error || new Error(options.message);
        },
      },
    );
    const result = await binding.projectWriteAllEntrypointsToDisk(project);
    const issues = (result.issues || []).filter((issue) =>
      ["fatal", "error", "bug"].includes(issue.severity),
    );
    assert.deepEqual(issues, [], "Native build reported errors");
    assert.equal(result.appPaths.length, entries.length);
    fs.writeFileSync(
      path.join(runDir, "build.json"),
      JSON.stringify({ config, appPaths: result.appPaths }, null, 2),
    );
    for (const file of walk(outputDir).filter((file) => file.endsWith(".js"))) {
      const code = fs.readFileSync(file, "utf8");
      try {
        new vm.Script(code, { filename: file });
      } catch (error) {
        // `new URL(..., import.meta.url)` also emits the original worker source as a static
        // asset. It can be a valid ESM file; runtime assets are parsed again as scripts when run.
        try {
          acorn.parse(code, { ecmaVersion: "latest", sourceType: "module" });
        } catch {
          throw error;
        }
      }
    }
    const endpoints = result.appPaths.map((endpoint, index) => ({
      entry: entries[index],
      paths: endpoint.clientPaths,
    }));
    const syncPaths = endpoints[0].paths.filter((file) => file.endsWith(".js"));
    const bootstrap = fs.readFileSync(
      path.join(outputDir, syncPaths[syncPaths.length - 1]),
      "utf8",
    );
    if (!variant.minify) {
      assert.ok(
        (bootstrap.match(/\.push\(\[/g) || []).length >= 2,
        "The fixture must inline multiple initial JS chunks",
      );
      const ids = JSON.parse(
        bootstrap.match(/"runtimeModuleIds":(\[[^\n]*\])\}/)[1],
      );
      assert.equal(
        typeof ids[ids.length - 1],
        development || variant.moduleIds === "named" ? "string" : "number",
        "Development uses named IDs for HMR; production follows the configured strategy",
      );
    }
    assert.ok(endpoints[0].paths.some((file) => file.endsWith(".css")));
    if (variant.sharedRuntime && !development) {
      assert.equal(
        syncPaths.length,
        2,
        "Shared runtime and exporting entry bootstrap",
      );
      const runtime = fs.readFileSync(
        path.join(outputDir, syncPaths[0]),
        "utf8",
      );
      assert.ok(!runtime.includes("__entryRegistration__"));
      assert.equal(
        new Set(
          endpoints.map((endpoint) =>
            endpoint.paths.find((file) => file.endsWith(".js")),
          ),
        ).size,
        1,
      );
    }
    return { outputDir, endpoints };
  } finally {
    if (project) await binding.projectShutdown(project);
  }
}

function createRealm(outputDir, dispatch, legacy, worker = false) {
  const appended = [];
  const errors = [];
  const scripts = [];
  const workers = [];
  const context = {
    URL,
    setTimeout,
    clearTimeout,
    queueMicrotask,
    console: {
      ...console,
      error(...args) {
        errors.push(args.map(String).join(" "));
      },
    },
    location: {
      href: "https://example.test/index.html",
      origin: "https://example.test",
    },
    document: {
      currentScript: undefined,
      querySelectorAll: () => [],
      getElementsByTagName: () => scripts,
      createElement(tagName) {
        return {
          tagName,
          addEventListener() {},
          getAttribute(name) {
            return this[name] || null;
          },
        };
      },
      head: {
        appendChild(element) {
          appended.push(element.src || element.href);
          if (element.tagName === "script") scripts.push(element);
          queueMicrotask(() => {
            try {
              if (element.src === "https://example.test/external.js") {
                context.EntryExportScript = { answer: 42 };
              } else if (element.tagName === "script") {
                execute(
                  new URL(element.src, context.location.href).pathname.slice(
                    "/assets/".length,
                  ),
                );
              }
              element.onload?.();
            } catch (error) {
              errors.push(String(error));
              element.onerror?.(error);
            }
          });
          return element;
        },
      },
    },
  };
  context.self = context;
  context.window = context;
  if (worker) {
    delete context.document;
    delete context.window;
    context.WorkerGlobalScope = function WorkerGlobalScope() {};
    Object.setPrototypeOf(context, context.WorkerGlobalScope.prototype);
    context.importScripts = (...urls) => {
      for (const url of urls) {
        const pathname = new URL(url, context.location.href).pathname;
        assert.ok(pathname.startsWith("/assets/"));
        execute(pathname.slice("/assets/".length));
      }
    };
  } else {
    context.Worker = class Worker {
      constructor(url) {
        const realm = createRealm(outputDir, "global", legacy, true);
        this.realm = realm;
        workers.push(realm);
        realm.context.location = {
          href: String(url),
          origin: new URL(url).origin,
        };
        realm.context.postMessage = (data) => {
          queueMicrotask(() => this.onmessage?.({ data }));
        };
        const pathname = new URL(url).pathname;
        assert.ok(pathname.startsWith("/assets/"));
        assert.ok(path.basename(pathname).startsWith("turbopack-worker-"));
        realm.execute(pathname.slice("/assets/".length));
        assert.equal(realm.context[exportName].entry, "worker");
        assert.equal(realm.context.TURBOPACK_NEXT_CHUNK_URLS.length, 0);
      }

      postMessage(data) {
        queueMicrotask(() => {
          Promise.resolve()
            .then(() => this.realm.context.onmessage({ data }))
            .catch((error) => {
              this.realm.errors.push(String(error));
              this.onerror?.(error);
            });
        });
      }

      terminate() {}
    };
  }
  if (legacy) context.globalThis = undefined;
  if (dispatch === "commonjs") {
    context.module = { exports: {} };
    context.exports = context.module.exports;
  } else if (dispatch === "exports") {
    context.exports = {};
  }
  vm.createContext(context);
  if (worker) {
    // Contextification wraps the sandbox's global object; set its worker prototype inside the realm.
    vm.runInContext(
      "Object.setPrototypeOf(self, WorkerGlobalScope.prototype)",
      context,
    );
  }
  function execute(file) {
    const previousScript = context.document?.currentScript;
    const script = {
      src: `https://example.test/assets/${file}`,
      getAttribute(name) {
        // Browsers keep the original relative attribute while `.src` resolves to an absolute URL.
        if (name === "src") return `/assets/${file}`;
        return this[name] || null;
      },
    };
    scripts.push(script);
    if (context.document) context.document.currentScript = script;
    try {
      new vm.Script(fs.readFileSync(path.join(outputDir, file), "utf8"), {
        filename: file,
      }).runInContext(context);
    } finally {
      if (context.document) context.document.currentScript = previousScript;
    }
  }
  return {
    context,
    appended,
    errors,
    workers,
    execute,
    exports() {
      if (dispatch === "commonjs") return context.module.exports;
      if (dispatch === "exports") return context.exports[exportName];
      return context[exportName];
    },
  };
}

async function waitFor(promise, label, realm) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error(
                `Timed out: ${label}; ${JSON.stringify({ errors: realm.errors, requests: realm.appended })}`,
              ),
            ),
          5000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function checkEntry(realm, entry) {
  const exposed = realm.exports();
  assert.ok(exposed, `No exports for ${entry}`);
  const asynchronous = entry === "async" || entry === "script";
  assert.equal(
    typeof exposed.then === "function",
    asynchronous,
    `${entry} export contract`,
  );
  const namespace = await waitFor(exposed, entry, realm);
  assert.equal(
    namespace.entry,
    entry,
    "Each bootstrap must export its own entry",
  );
  if (asynchronous) {
    assert.equal(namespace.answer, 42);
  } else {
    assert.equal(namespace.value, 17);
    assert.equal(namespace.executions, 1);
    if (entry === "sync-a") {
      assert.equal(namespace.default.entry, "sync-a");
      const previous = namespace.live;
      assert.equal(namespace.bump(), previous + 1);
      assert.equal(
        namespace.live,
        previous + 1,
        "Reexports retain live bindings",
      );
      assert.equal(
        await waitFor(namespace.load(), "dynamic import", realm),
        `dynamic:${previous + 1}`,
      );
      const workerResult = await waitFor(
        namespace.startWorker(),
        "worker",
        realm,
      );
      assert.deepEqual(JSON.parse(JSON.stringify(workerResult)), {
        entry: "worker",
        executions: 1,
        live: 1,
        dynamic: "dynamic:1",
      });
      assert.deepEqual(
        realm.workers.flatMap((worker) => worker.errors),
        [],
      );
    }
  }
  return namespace;
}

async function verifyCase(build, variant) {
  const results = [];
  for (const dispatch of ["global", "commonjs", "exports"]) {
    for (const endpoint of build.endpoints) {
      const realm = createRealm(build.outputDir, dispatch, variant.legacy);
      for (const file of endpoint.paths.filter((file) =>
        file.endsWith(".js"),
      )) {
        realm.execute(file);
      }
      if (endpoint.entry.startsWith("sync")) {
        assert.deepEqual(
          realm.appended.filter((file) => file.endsWith(".js")),
          [],
          "Synchronous exports must not wait for initial JS chunk requests",
        );
      }
      await checkEntry(realm, endpoint.entry);
      await new Promise((resolve) => setTimeout(resolve, 0));
      assert.deepEqual(realm.errors, []);
      results.push(`${dispatch}:${endpoint.entry}`);
    }
    const realm = createRealm(build.outputDir, dispatch, variant.legacy);
    const loaded = new Set();
    const captured = [];
    for (const endpoint of build.endpoints) {
      for (const file of endpoint.paths.filter((file) =>
        file.endsWith(".js"),
      )) {
        if (!loaded.has(file)) {
          loaded.add(file);
          realm.execute(file);
        }
      }
      captured.push(await checkEntry(realm, endpoint.entry));
    }
    assert.equal(
      captured[0].live,
      captured[1].live,
      "Shared live exports stay connected",
    );
    assert.equal(realm.context.__entryExportSharedExecutions, 1);
    assert.equal(
      realm.appended.filter(
        (file) => file === "https://example.test/external.js",
      ).length,
      1,
    );
    assert.deepEqual(realm.errors, []);
    results.push(`${dispatch}:combined`);
  }
  return results;
}

async function main() {
  const binding = loadBinding();
  fs.mkdirSync(path.join(repoRoot, "target"), { recursive: true });
  const output = fs.mkdtempSync(
    path.join(repoRoot, "target/entry-root-export-regression-"),
  );
  const variants = [
    ...["named", "deterministic"].flatMap((moduleIds) =>
      [false, true].map((sharedRuntime) => ({
        mode: "production",
        moduleIds,
        sharedRuntime,
      })),
    ),
    {
      mode: "production",
      moduleIds: "deterministic",
      sharedRuntime: true,
      minify: true,
    },
    {
      mode: "production",
      moduleIds: "named",
      sharedRuntime: true,
      legacy: true,
    },
    {
      mode: "production",
      moduleIds: "deterministic",
      sharedRuntime: false,
      legacy: true,
    },
    { mode: "development", moduleIds: "named", sharedRuntime: false },
    { mode: "development", moduleIds: "deterministic", sharedRuntime: true },
  ];
  const report = [];
  for (const [index, variant] of variants.entries()) {
    const runDir = path.join(output, String(index));
    fs.mkdirSync(runDir);
    const built = await buildCase(binding, runDir, variant);
    report.push({ ...variant, checks: await verifyCase(built, variant) });
    console.log(`Verified entryRootExport ${JSON.stringify(variant)}`);
  }
  fs.writeFileSync(
    path.join(output, "report.json"),
    JSON.stringify(report, null, 2),
  );
  console.log(`Entry exports regression passed: ${output}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
