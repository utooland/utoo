import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const projectPath = process.argv[2];
if (!projectPath || !path.isAbsolute(projectPath)) {
  throw new Error("An absolute fixture directory is required");
}
const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../..",
);

type RegExpResult = {
  word: string;
  matched: string;
  rejectsDifferentWord: boolean;
  replacement: string;
};

async function main() {
  // The override belongs to pack's addon only. HtmlPlugin's domparser-rs
  // dependency must load its own native library.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { build } = await import("../commands/build");
  const sourceDir = path.join(projectPath, "src");
  const outputDir = path.join(projectPath, "dist");
  fs.mkdirSync(sourceDir, { recursive: true });
  fs.writeFileSync(
    path.join(sourceDir, "regexp-dependency.js"),
    `export function inspectRegExp(value) {
  const pattern = /^(?<word>[a-z]+)-\\k<word>$/;
  const match = pattern.exec(value);
  return {
    word: match.groups.word,
    matched: match[0],
    rejectsDifferentWord: !pattern.test("hello-world"),
    replacement: value.replace(pattern, "[$<word>]"),
  };
}
`,
  );
  fs.writeFileSync(
    path.join(sourceDir, "index.js"),
    'export { inspectRegExp } from "./regexp-dependency.js";\n',
  );

  await build(
    {
      buildId: "swc-helpers-test",
      tracing: false,
      config: {
        mode: "production",
        target: "chrome 49",
        entry: [
          {
            name: "index",
            import: "./src/index.js",
            library: { name: "SwcHelpersRegression" },
          },
        ],
        output: { path: outputDir, filename: "index.js", clean: true },
        sourceMaps: false,
        persistentCaching: false,
        optimization: { minify: false },
      },
    },
    projectPath,
    repoRoot,
  );

  const filename = path.join(outputDir, "index.js");
  const body = fs.readFileSync(filename, "utf8");
  // Modern Node supports the original syntax. Require an emitted helper so
  // native RegExp support cannot hide a missing compatibility transform.
  assert.ok(body.includes("_wrap_reg_exp"), "Missing emitted RegExp helper");
  const context: {
    URL: typeof URL;
    self?: unknown;
    SwcHelpersRegression?: { inspectRegExp: (value: string) => RegExpResult };
  } = { URL };
  context.self = context;
  vm.runInNewContext(body, context, { filename, timeout: 1000 });
  assert.ok(context.SwcHelpersRegression, "Missing library exports");
  const result = context.SwcHelpersRegression.inspectRegExp("hello-hello");
  console.log(`__SWC_HELPERS__${JSON.stringify(result)}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
