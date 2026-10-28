import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { build } from "../commands/build";

async function main() {
  const projectPath = process.argv[2];
  if (!projectPath || !path.isAbsolute(projectPath)) {
    throw new Error("An absolute fixture directory is required");
  }

  fs.mkdirSync(path.join(projectPath, "input"), { recursive: true });
  fs.writeFileSync(
    path.join(projectPath, "input/index.js"),
    `function RootWidget() { return "default-widget"; }
RootWidget.nested = {
  Widget: function NestedWidget() { return "nested-widget"; }
};
export default RootWidget;
`,
  );

  async function buildLibrary(
    directory: string,
    libraryName: string,
    exportPath: string[],
  ) {
    const outputPath = path.join(projectPath, directory);
    await build(
      {
        buildId: "library-content-hash-test",
        tracing: false,
        config: {
          entry: [
            {
              name: "index",
              import: "./input/index.js",
              library: { name: libraryName, export: exportPath },
            },
          ],
          mode: "production",
          persistentCaching: false,
          cacheDirectory: path.join(projectPath, ".cache"),
          sourceMaps: false,
          output: {
            path: outputPath,
            filename: "index.[contenthash:8].umd.min.js",
          },
          optimization: { minify: true, moduleIds: "named" },
        },
      },
      projectPath,
      projectPath,
    );

    const scripts = fs
      .readdirSync(outputPath)
      .filter((file) => file.endsWith(".js"));
    if (scripts.length !== 1) {
      throw new Error("Expected one self-contained library script");
    }
    const filename = scripts[0];
    const body = fs.readFileSync(path.join(outputPath, filename), "utf8");
    const context: Record<string, unknown> = { URL };
    vm.runInNewContext(body, context, { timeout: 1000 });
    const exported = context[libraryName];

    return {
      filename,
      body,
      exportedType: typeof exported,
      exportedValue: typeof exported === "function" ? exported() : undefined,
    };
  }

  const first = await buildLibrary("first", "WidgetBuildA", ["default"]);
  const repeated = await buildLibrary("repeated", "WidgetBuildA", ["default"]);
  const renamed = await buildLibrary("renamed", "WidgetBuildB", ["default"]);
  const changedExport = await buildLibrary("changed-export", "WidgetBuildA", [
    "default",
    "nested",
    "Widget",
  ]);

  console.log(
    `__LIBRARY_CONTENT_HASH__${JSON.stringify({
      sameConfigStableFilename: first.filename === repeated.filename,
      sameConfigStableBody: first.body === repeated.body,
      libraryNameChangesFilename: first.filename !== renamed.filename,
      libraryNameChangesBody: first.body !== renamed.body,
      exportPathChangesFilename: first.filename !== changedExport.filename,
      exportPathChangesBody: first.body !== changedExport.body,
      exports: [first, repeated, renamed, changedExport].map((result) => ({
        type: result.exportedType,
        value: result.exportedValue,
      })),
    })}`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
