import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";
import type { ConfigComplete, HtmlConfig } from "../config/types";
import { getInitialAssetsFromStats } from "../utils/getInitialAssets";

const [, , projectPath, scenario] = process.argv;
if (!projectPath || !["production", "development", "node"].includes(scenario)) {
  throw new Error("Usage: sharedRuntimeChild <projectPath> <scenario>");
}
const outputDir = path.join(projectPath, "dist");
const publicPath = "/nested/assets/";

function outputHashes(directory: string): Record<string, string> {
  const result: Record<string, string> = {};
  function visit(current: string) {
    for (const item of fs.readdirSync(current, { withFileTypes: true })) {
      const file = path.join(current, item.name);
      if (item.isDirectory()) visit(file);
      else {
        result[path.relative(directory, file)] = createHash("sha256")
          .update(fs.readFileSync(file))
          .digest("hex");
      }
    }
  }
  visit(directory);
  return result;
}

function htmlScripts(name: string): string[] {
  const html = fs.readFileSync(path.join(outputDir, `${name}.html`), "utf8");
  const scripts = [...html.matchAll(/<script\b[^>]*\bsrc="([^"]+)"/g)].map(
    (match) => match[1],
  );
  assert.ok(scripts.length > 0, `No script assets in ${name}.html`);
  for (const script of scripts) {
    assert.ok(script.startsWith(publicPath), `Incorrect publicPath: ${script}`);
    assert.ok(
      fs.existsSync(path.join(outputDir, script.slice(publicPath.length))),
      `HTML references a missing asset: ${script}`,
    );
  }
  return scripts;
}

async function main() {
  // The override belongs to pack's addon only. Other napi-rs packages, such as
  // domparser-rs used by HtmlPlugin, must load their own native libraries.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { build } = await import("../commands/build");
  const sources = {
    "shared.js": 'export const value = "shared-value";\n',
    "dynamic.js": 'export default "dynamic-value";\n',
    "worker.js":
      'import("./dynamic.js").then(({ default: value }) => postMessage(value));\n',
    "app.js": `import { value } from "./shared.js";
globalThis.sharedValue = value;
import("./dynamic.js").then(({ default: value }) => { globalThis.dynamicValue = value; });
new Worker(new URL("./worker.js", import.meta.url));
`,
    "first.js": 'import "./app.js";\nconsole.log("first-entry");\n',
    "second.js": 'import "./app.js";\nconsole.log("second-entry");\n',
    "node.js": `import { value } from "./shared.js";
import("./dynamic.js").then(({ default: dynamic }) => console.log(value + ":" + dynamic));
`,
  };
  fs.mkdirSync(path.join(projectPath, "src"), { recursive: true });
  for (const [name, content] of Object.entries(sources)) {
    fs.writeFileSync(path.join(projectPath, "src", name), content);
  }
  const config: ConfigComplete & { html?: HtmlConfig } = {
    mode: scenario === "development" ? "development" : "production",
    target: scenario === "node" ? "current node" : undefined,
    entry:
      scenario === "node"
        ? [{ name: "node", import: "./src/node.js" }]
        : ["first", "second"].map((name) => ({
            name,
            import: `./src/${name}.js`,
            html: { filename: `${name}.html` },
          })),
    html: scenario === "node" ? undefined : { filename: "all.html" },
    output: {
      path: outputDir,
      clean: true,
      publicPath,
      ...(scenario === "node" ? { filename: "node.js" } : {}),
    },
    sourceMaps: false,
    stats: scenario === "production",
    persistentCaching: false,
    optimization: { concatenateModules: true },
  };

  async function compile(sharedRuntime?: boolean) {
    await build(
      {
        config: {
          ...config,
          optimization: { ...config.optimization, sharedRuntime },
        },
        dev: scenario === "development",
        tracing: false,
        buildId: "shared-runtime-test",
      },
      projectPath,
      projectPath,
    );
  }

  await compile();
  const baseline = outputHashes(outputDir);
  if (scenario === "production") {
    const first = htmlScripts("first");
    const second = htmlScripts("second");
    assert.equal(
      first.length,
      1,
      "Default output must keep its embedded runtime",
    );
    assert.equal(second.length, 1);
    assert.notEqual(first[0], second[0]);
  }

  await compile(true);
  if (scenario === "production") {
    const first = htmlScripts("first");
    const second = htmlScripts("second");
    const all = htmlScripts("all");
    assert.equal(
      first.length,
      2,
      "Entry HTML must include bootstrap and runtime",
    );
    assert.equal(second.length, 2);
    assert.notEqual(
      first[0],
      second[0],
      "Entry bootstraps must remain distinct",
    );
    assert.equal(
      first[1],
      second[1],
      "The runtime must be shared and loaded last",
    );
    assert.deepEqual(all, [first[0], first[1], second[0]]);

    const runtime = first[1].slice(publicPath.length);
    const stats = JSON.parse(
      fs.readFileSync(path.join(outputDir, "stats.json"), "utf8"),
    ) as {
      entrypoints: Record<
        string,
        { assets: { name: string }[]; chunks: string[] }
      >;
      chunks: { id: string; files: string[] }[];
    };
    const runtimeChunks = stats.chunks.filter((chunk) =>
      chunk.files.includes(runtime),
    );
    assert.equal(
      runtimeChunks.length,
      1,
      "Stats must contain the shared runtime",
    );
    for (const [name, scripts] of [
      ["first", first],
      ["second", second],
    ] as const) {
      const entrypoint = stats.entrypoints[name];
      const assets = entrypoint.assets.map((asset) => asset.name);
      const bootstrap = scripts[0].slice(publicPath.length);
      assert.deepEqual(assets.slice(-2), [bootstrap, runtime]);
      assert.deepEqual(entrypoint.chunks.slice(-2), [
        bootstrap,
        runtimeChunks[0].id,
      ]);
    }
    assert.ok(getInitialAssetsFromStats(outputDir).js.includes(runtime));
  } else {
    assert.deepEqual(outputHashes(outputDir), baseline);
    if (scenario === "node") {
      const { stdout } = await promisify(execFile)(process.execPath, [
        path.join(outputDir, "node.js"),
      ]);
      assert.equal(stdout.trim(), "shared-value:dynamic-value");
    }
  }
  console.log(`__SHARED_RUNTIME__${JSON.stringify({ scenario })}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
