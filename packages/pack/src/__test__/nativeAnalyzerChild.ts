import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import type { ConfigComplete } from "../config/types";

const [, , projectPath, scenario] = process.argv;
assert(projectPath && ["single", "mpa", "html"].includes(scenario));

type Edges = { offset: number; length: number };
type ModuleHeader = {
  module_index_hash: string;
  modules: { ident: string; path: string }[];
  module_dependencies: Edges;
  async_module_dependencies: Edges;
};
type AnalyzeHeader = {
  module_index_hash: string;
  sources: { parent_source_index: number | null; path: string }[];
  chunk_parts: { source_index: number; size: number }[];
  output_files: { filename: string }[];
  route_entries: { module_ident: string; module_path: string }[];
};

// Decode the public artifact format independently of the UI. Both JSON headers
// and adjacency arrays use a big-endian u32 length, followed by their payload.
function readArtifact<T>(filename: string): { header: T; binary: Buffer } {
  const contents = fs.readFileSync(filename);
  const jsonLength = contents.readUInt32BE(0);
  assert(jsonLength > 0 && jsonLength + 4 <= contents.length);
  return {
    header: JSON.parse(contents.subarray(4, jsonLength + 4).toString("utf8")),
    binary: contents.subarray(jsonLength + 4),
  };
}

function readEdges(binary: Buffer, reference: Edges, index: number): number[] {
  if (reference.length === 0) return [];
  const count = binary.readUInt32BE(reference.offset);
  if (index >= count) return [];
  const offsets = reference.offset + 4;
  const start =
    index === 0 ? 0 : binary.readUInt32BE(offsets + (index - 1) * 4);
  const end = binary.readUInt32BE(offsets + index * 4);
  return Array.from({ length: end - start }, (_, i) =>
    binary.readUInt32BE(offsets + count * 4 + (start + i) * 4),
  );
}

function sourcePaths(header: AnalyzeHeader): string[] {
  const fullPath = (index: number): string => {
    const source = header.sources[index];
    return source.parent_source_index === null
      ? source.path
      : fullPath(source.parent_source_index) + source.path;
  };
  return header.sources.map((_, index) => fullPath(index));
}

function assertSource(header: AnalyzeHeader, suffix: string) {
  const indices = new Set(
    sourcePaths(header).flatMap((name, index) =>
      name.endsWith(suffix) ? [index] : [],
    ),
  );
  assert(indices.size > 0, `Missing source attribution for ${suffix}`);
  assert(
    header.chunk_parts.some(
      (part) => indices.has(part.source_index) && part.size > 0,
    ),
    `Missing emitted bytes for ${suffix}`,
  );
}

function hasImportPath(
  graph: ReturnType<typeof readArtifact<ModuleHeader>>,
  from: string,
  to: string,
  requireAsync = false,
): boolean {
  const roots = graph.header.modules.flatMap((module, index) =>
    module.path.endsWith(from) ? [{ index, async: false }] : [],
  );
  assert(roots.length > 0, `Missing graph root ${from}`);
  const visited = new Set<string>();
  for (let position = 0; position < roots.length; position++) {
    const current = roots[position];
    const key = `${current.index}:${current.async}`;
    if (visited.has(key)) continue;
    visited.add(key);
    if (
      graph.header.modules[current.index].path.endsWith(to) &&
      (!requireAsync || current.async)
    ) {
      return true;
    }
    for (const index of readEdges(
      graph.binary,
      graph.header.module_dependencies,
      current.index,
    )) {
      roots.push({ index, async: current.async });
    }
    for (const index of readEdges(
      graph.binary,
      graph.header.async_module_dependencies,
      current.index,
    )) {
      roots.push({ index, async: true });
    }
  }
  return false;
}

function readFiles(directory: string): Record<string, string> {
  return Object.fromEntries(
    fs.readdirSync(directory, { recursive: true }).flatMap((item) => {
      const filename = path.join(directory, item);
      return fs.statSync(filename).isFile()
        ? [[item, fs.readFileSync(filename).toString("base64")]]
        : [];
    }),
  );
}

async function main() {
  // This override is Pack-specific; domparser-rs must use its own native addon.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { analyze } = await import("../commands/analyze");
  const { build } = await import("../commands/build");
  const sources: Record<string, string> = {
    "alpha.js": [
      'import { shared } from "./shared.js";',
      'import { alpha } from "./alpha-only.js";',
      'import { compiled } from "./compiled-widget.js";',
      'import "./alpha.css";',
      "globalThis.alpha = [shared, alpha, compiled];",
      'globalThis.loadLazy = () => import("./lazy.js");',
    ].join("\n"),
    "beta.js": [
      'import { shared } from "./shared.js";',
      'import { beta } from "./beta-only.js";',
      'import "./beta.css";',
      "globalThis.beta = [shared, beta];",
    ].join("\n"),
    "shared.js": 'export const shared = "shared-between-pages";',
    "alpha-only.js": 'export const alpha = "alpha-private-module";',
    "beta-only.js": 'export const beta = "beta-private-module";',
    "lazy.js": 'export const lazy = "alpha-lazy-module";',
    "compiled-widget.js": [
      'export const compiled = "precompiled-widget";',
      "//# sourceMappingURL=compiled-widget.js.map",
    ].join("\n"),
    "compiled-widget.js.map": JSON.stringify({
      version: 3,
      file: "compiled-widget.js",
      sources: ["original-widget.ts"],
      sourcesContent: ['export const compiled: string = "precompiled-widget";'],
      names: [],
      mappings: "AAAA",
    }),
    "alpha.css": ".alpha { color: red; }",
    "beta.css": ".beta { color: blue; }",
  };
  const inputPath = path.join(projectPath, "src");
  fs.mkdirSync(inputPath, { recursive: true });
  fs.writeFileSync(path.join(projectPath, "package.json"), '{"private":true}');
  for (const [filename, contents] of Object.entries(sources)) {
    fs.writeFileSync(path.join(inputPath, filename), contents);
  }
  const originalHtml =
    '<!doctype html><html><head><title>HTML entry</title></head><body><div id="app"></div><script type="module" src="./src/alpha.js"></script></body></html>';
  fs.writeFileSync(path.join(projectPath, "index.html"), originalHtml);

  const distPath = path.join(projectPath, "dist");
  fs.mkdirSync(distPath, { recursive: true });
  fs.writeFileSync(path.join(distPath, "stats.json"), '{"existingStats":true}');
  fs.writeFileSync(path.join(distPath, "keep.js"), 'console.log("keep");');
  const existingOutputs = readFiles(distPath);
  const config: ConfigComplete = {
    mode: "production",
    entry: [
      {
        name: "alpha",
        import: scenario === "html" ? "./index.html" : "./src/alpha.js",
        html: {},
      },
      ...(scenario === "mpa"
        ? [{ name: "nested/beta", import: "./src/beta.js", html: {} }]
        : []),
    ],
    output: {
      path: distPath,
      filename: "[name].[contenthash:8].js",
      chunkFilename: "chunks/[name].[contenthash:8].js",
      clean: true,
    },
    stats: true,
    sourceMaps: false,
    persistentCaching: false,
    optimization: { minify: true },
  };
  const originalConfig = JSON.stringify(config);
  const reportPath = path.join(projectPath, "report");
  const previousAnalyze = process.env.ANALYZE;
  process.env.ANALYZE = "true";
  let result: Awaited<ReturnType<typeof analyze>>;
  try {
    result = await analyze(
      { config, tracing: false },
      projectPath,
      projectPath,
      { outputPath: reportPath },
    );
  } finally {
    if (previousAnalyze === undefined) delete process.env.ANALYZE;
    else process.env.ANALYZE = previousAnalyze;
  }
  assert.equal(result.outputPath, reportPath);
  assert.equal(
    JSON.stringify(config),
    originalConfig,
    "Caller config was mutated",
  );
  assert.equal(
    fs.readFileSync(path.join(projectPath, "index.html"), "utf8"),
    originalHtml,
    "Analysis changed the HTML entry",
  );
  assert.deepEqual(
    readFiles(distPath),
    existingOutputs,
    "Analysis changed dist",
  );
  assert.deepEqual(
    result.entries.map((entry) => entry.name).sort(),
    scenario === "mpa" ? ["alpha", "nested/beta"] : ["alpha"],
  );
  assert.deepEqual(
    JSON.parse(
      fs.readFileSync(path.join(reportPath, "data/entries.json"), "utf8"),
    ),
    result.entries,
  );
  assert(
    fs.existsSync(path.join(reportPath, "index.html")),
    "Report UI missing",
  );
  assert(
    !Object.keys(readFiles(reportPath)).some((name) =>
      name.endsWith("stats.json"),
    ),
    "Native analysis must not generate Webpack stats",
  );

  const graph = readArtifact<ModuleHeader>(
    path.join(reportPath, "data/modules.data"),
  );
  assert(graph.header.module_index_hash, "Missing graph identity");
  const entryReports = new Map(
    result.entries.map((entry) => [
      entry.name,
      readArtifact<AnalyzeHeader>(
        path.join(
          reportPath,
          "data",
          entry.path.replace(/^\//, ""),
          "analyze.data",
        ),
      ).header,
    ]),
  );
  const alpha = entryReports.get("alpha");
  assert(alpha);
  for (const [name, report] of entryReports) {
    assert.equal(report.module_index_hash, graph.header.module_index_hash);
    assert(report.output_files.length > 0, `No outputs for ${name}`);
    assertSource(report, "/src/shared.js");
    const ownModule = name === "alpha" ? "alpha" : "beta";
    const otherModule = name === "alpha" ? "beta" : "alpha";
    assertSource(report, `/src/${ownModule}.js`);
    assertSource(report, `/src/${ownModule}-only.js`);
    assertSource(report, `/src/${ownModule}.css`);
    assert(
      !sourcePaths(report).some((source) =>
        source.endsWith(`/src/${otherModule}-only.js`),
      ),
      `Entry ${name} included another entry's private module`,
    );
    assert(
      report.route_entries.some((entry) =>
        entry.module_path.endsWith(`/src/${ownModule}.js`),
      ),
      `Entry ${name} lacks its exact module root`,
    );
    assert(
      !report.route_entries.some((entry) =>
        entry.module_path.endsWith(`/src/${otherModule}.js`),
      ),
    );
  }
  assertSource(alpha, "/src/lazy.js");
  // Original input maps must not break the join between the source treemap and
  // resolved module graph. The actual imported JS is the analyzer's identity.
  assertSource(alpha, "/src/compiled-widget.js");
  assert(
    !sourcePaths(alpha).some((source) =>
      source.endsWith("/original-widget.ts"),
    ),
  );
  assert(hasImportPath(graph, "/src/alpha.js", "/src/shared.js"));
  assert(hasImportPath(graph, "/src/alpha.js", "/src/compiled-widget.js"));
  assert(hasImportPath(graph, "/src/alpha.js", "/src/lazy.js", true));
  if (scenario === "mpa") {
    const beta = entryReports.get("nested/beta");
    assert(beta);
    assert(
      !sourcePaths(beta).some((source) => source.endsWith("/src/lazy.js")),
    );
    assert(hasImportPath(graph, "/src/beta.js", "/src/shared.js"));
    assert(!hasImportPath(graph, "/src/beta.js", "/src/alpha-only.js"));

    // The analyzer's private config must not disable stats on a later build.
    delete process.env.ANALYZE;
    await build({ config, tracing: false }, projectPath, projectPath);
    const stats = JSON.parse(
      fs.readFileSync(path.join(distPath, "stats.json"), "utf8"),
    );
    assert.deepEqual(Object.keys(stats.entrypoints).sort(), [
      "alpha",
      "nested/beta",
    ]);
    assert(stats.assets.length > 0);
    assert(
      !stats.assets.some((asset: { name: string }) =>
        asset.name.endsWith(".map"),
      ),
      "Analysis must not enable source maps for a later ordinary build",
    );
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
