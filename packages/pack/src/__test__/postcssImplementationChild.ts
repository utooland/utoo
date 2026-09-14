import assert from "assert/strict";
import fs from "fs";
import { createRequire } from "module";
import path from "path";
import { build, serve } from "../index";

const require = createRequire(import.meta.url);
const [, , projectPath, scenario] = process.argv;
if (!projectPath || !scenario) throw new Error("Missing fixture arguments");

const cssPath = path.join(projectPath, "style.css");
const outputPath = path.join(projectPath, "dist");
// The framework-owned processor deliberately lives outside the application.
const implementationPath = path.join(
  projectPath,
  "../",
  `${path.basename(projectPath)}.cjs`,
);

function writeCss(value: string) {
  fs.writeFileSync(cssPath, `.example { --value: ${value}; }\n`);
}

function readOutput(directory: string): string {
  if (!fs.existsSync(directory)) return "";
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .map((entry) => {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) return readOutput(file);
      return /\.(css|js)$/.test(entry.name)
        ? fs.readFileSync(file, "utf8")
        : "";
    })
    .join("\n");
}

function hasTransformedCss(value: string) {
  const output = readOutput(outputPath).replace(/\s+/g, "");
  return (
    output.includes(`--value:implementation-${value}`) &&
    output.includes("--from-config:yes") &&
    (scenario === "config-only" || output.includes("--from-inline:yes"))
  );
}

async function waitForCss(value: string) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (hasTransformedCss(value)) return true;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `Missing transformed CSS for ${value}: ${readOutput(outputPath)}`,
  );
}

async function main() {
  // A private fixture package simulates an incompatible application processor.
  // Never modify any installed dependency: this tree is created and removed by
  // the test, under a fresh temporary application directory.
  const ambientPath = path.join(projectPath, "node_modules/postcss");
  fs.mkdirSync(ambientPath, { recursive: true });
  fs.writeFileSync(
    path.join(projectPath, "package.json"),
    '{"private":true}\n',
  );
  fs.writeFileSync(
    path.join(ambientPath, "index.js"),
    'throw new Error("AMBIENT_POSTCSS_MUST_NOT_RUN");\n',
  );
  fs.writeFileSync(
    implementationPath,
    `
    const postcss = require(${JSON.stringify(require.resolve("postcss"))});
    module.exports = (plugins) => postcss([...plugins, {
      postcssPlugin: "explicit-implementation",
      Once(root) {
        root.walkDecls("--value", (decl) => { decl.value = "implementation-" + decl.value; });
      }
    }]);
  `,
  );
  fs.writeFileSync(
    path.join(projectPath, "postcss.config.js"),
    `
    module.exports = { plugins: [{
      postcssPlugin: "discovered-plugin",
      Once(root) { root.first.append({ prop: "--from-config", value: "yes" }); }
    }] };
  `,
  );
  const inlinePluginPath = path.join(projectPath, "inline-plugin.cjs");
  fs.writeFileSync(
    inlinePluginPath,
    `
    module.exports = () => ({
      postcssPlugin: "inline-plugin",
      Once(root) { root.first.append({ prop: "--from-inline", value: "yes" }); }
    });
  `,
  );
  writeCss("initial");
  fs.writeFileSync(
    path.join(projectPath, "index.js"),
    'import "./style.css";\n',
  );

  const config = {
    entry: [{ import: "./index.js", name: "main" }],
    output: { path: "./dist" },
    stats: false,
    persistentCaching: false,
    pluginRuntimeStrategy: "childProcesses" as const,
    optimization: { minify: false, packageImports: [] },
    styles: {
      ...(scenario === "inline" ? { inlineCss: {} } : {}),
      postcss: {
        ...(scenario === "default"
          ? {}
          : {
              implementation:
                scenario === "missing"
                  ? path.join(projectPath, "missing-postcss.cjs")
                  : implementationPath,
            }),
        ...(scenario === "config-only"
          ? {}
          : { plugins: { [inlinePluginPath]: {} } }),
      },
    },
    ...(scenario === "server"
      ? {
          entry: [{ import: "./client.js", name: "client" }],
          server: { entry: "./index.js", output: { path: "./dist/server" } },
        }
      : {}),
  };
  fs.writeFileSync(
    path.join(projectPath, "client.js"),
    'console.log("client");\n',
  );

  if (scenario === "default" || scenario === "missing") {
    let error = "";
    try {
      await build({ config, tracing: false }, projectPath, projectPath);
    } catch (caught) {
      error = String(caught);
    }
    const expected =
      scenario === "default"
        ? "AMBIENT_POSTCSS_MUST_NOT_RUN"
        : "missing-postcss.cjs";
    assert.ok(
      error.includes(expected),
      `Expected ${expected}, received ${error}`,
    );
    return scenario === "default" ? { ambient: true } : { missing: true };
  }

  if (scenario === "dev") {
    await serve({ config, tracing: false }, projectPath, projectPath, {
      hostname: "127.0.0.1",
      port: 0,
      logServerInfo: false,
    });
    const initial = await waitForCss("initial");
    writeCss("updated");
    return { initial, updated: await waitForCss("updated") };
  }
  await build({ config, tracing: false }, projectPath, projectPath);
  return { initial: hasTransformedCss("initial") };
}

main()
  .then((result) => {
    fs.rmSync(implementationPath, { force: true });
    console.log(`__POSTCSS_IMPLEMENTATION__${JSON.stringify(result)}`);
    process.exit(0);
  })
  .catch((error) => {
    fs.rmSync(implementationPath, { force: true });
    console.error(error);
    process.exit(1);
  });
