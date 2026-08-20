const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const packRoot = path.dirname(
  require.resolve("@utoo/pack/package.json", { paths: [__dirname] }),
);
const evaluatorDir = path.join(packRoot, "cjs", ".turbopack");
const runtimePath = path.join(evaluatorDir, "_turbopack__runtime.js");

function collectJavaScriptFiles(directory) {
  if (!fs.existsSync(directory)) {
    return [];
  }

  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      return collectJavaScriptFiles(entryPath);
    }
    return entry.isFile() && entry.name.endsWith(".js") ? [entryPath] : [];
  });
}

async function main() {
  fs.rmSync(evaluatorDir, { recursive: true, force: true });
  fs.rmSync(path.join(__dirname, "regression-dist"), {
    recursive: true,
    force: true,
  });

  const { build } = require("@utoo/pack");
  let buildError;
  try {
    // Separate builds make the shared-runtime overwrite deterministic: the asynchronous PostCSS
    // graph emits first, then the synchronous loader graph emits to the same evaluator directory.
    await build({
      config: {
        mode: "development",
        entry: [{ name: "async", import: "./src/async.js" }],
        output: {
          path: "./regression-dist/async",
          clean: true,
        },
        pluginRuntimeStrategy: "workerThreads",
        sourceMaps: true,
      },
    });
    await build({
      config: {
        mode: "development",
        entry: [{ name: "sync", import: "./src/sync.js" }],
        output: {
          path: "./regression-dist/sync",
          clean: true,
        },
        module: {
          rules: {
            "*.sync-txt": {
              loaders: [require.resolve("./sync-loader.cjs")],
              as: "*.js",
            },
          },
        },
        pluginRuntimeStrategy: "workerThreads",
        sourceMaps: true,
      },
    });
  } catch (error) {
    buildError = error;
  }

  assert.ok(fs.existsSync(runtimePath), `missing runtime: ${runtimePath}`);
  const runtime = fs.readFileSync(runtimePath, "utf8");
  const asyncChunk = collectJavaScriptFiles(evaluatorDir).find(
    (file) =>
      file !== runtimePath &&
      fs.readFileSync(file, "utf8").includes("__turbopack_context__.a("),
  );

  assert.ok(asyncChunk, "expected PostCSS evaluation to emit an async chunk");
  assert.match(
    runtime,
    /contextPrototype\.a\s*=\s*asyncModule/,
    "the shared evaluator runtime must retain the async-module helper",
  );
  if (buildError) {
    throw buildError;
  }

  console.log(`verified async evaluator chunk: ${path.basename(asyncChunk)}`);
  console.log("verified shared runtime async-module helper");
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
