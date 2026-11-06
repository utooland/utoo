import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import type { ConfigComplete } from "../config/types";

const [, , projectPath, scenario] = process.argv;
assert(
  projectPath &&
    [
      "single",
      "mpa",
      "html",
      "node",
      "mixed",
      "server-only",
      "server-functions",
      "server-functions-entry",
    ].includes(scenario),
);

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
  output_file_modules: Edges;
  route_entries: {
    module_ident: string;
    module_path: string;
    entry_kind?: string;
  }[];
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
  scope?: { roots: Set<string>; members: Set<number> },
): boolean {
  const roots = graph.header.modules.flatMap((module, index) =>
    module.path.endsWith(from) && (!scope || scope.roots.has(module.ident))
      ? [{ index, async: false }]
      : [],
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
      (!requireAsync || current.async) &&
      (!scope || scope.members.has(current.index))
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

async function serverScenario(
  analyze: typeof import("../commands/analyze").analyze,
  build: typeof import("../commands/build").build,
) {
  const withFunctionEntry = scenario === "server-functions-entry";
  const inputPath = path.join(projectPath, "src");
  const sources: Record<string, string> = {
    "client.js":
      'import { shared } from "./shared.js"; import { client } from "./client-only.js"; globalThis.client = [shared, client];',
    "client-only.js": 'export const client = "browser-only-value";',
    "server-main.js":
      'import { shared } from "./shared.js"; import { server } from "./server-only.js"; globalThis.server = [shared, server]; globalThis.loadServer = () => import("./server-lazy.js");',
    "server-admin.js":
      'import { shared } from "./shared.js"; import { admin } from "./admin-only.js"; globalThis.admin = [shared, admin];',
    "server-only.js":
      'export const server = "server-only-value";\n//# sourceMappingURL=server-only.js.map',
    "server-only.js.map": JSON.stringify({
      version: 3,
      file: "server-only.js",
      sources: ["original-server.ts"],
      sourcesContent: ['export const server: string = "server-only-value";'],
      names: [],
      mappings: "AAAA",
    }),
    "admin-only.js": 'export const admin = "admin-only-value";',
    "server-lazy.js": 'export const lazy = "server-lazy-value";',
    "shared.js": 'export const shared = "shared-in-each-environment";',
    "function-client.js":
      'import { shared } from "./shared.js"; import { perform } from "./actions.js"; globalThis.run = () => perform(shared);',
    "actions.js":
      '"use server"; import { secret } from "./action-private.js"; import { shared } from "./shared.js"; export async function perform(value) { return secret + shared + value; }',
    "action-private.js": 'export const secret = "server-action-secret";',
    "transport.js":
      "export function createServerReference(id, name) { return async (...args) => ({ id, name, args }); }",
    "register.js":
      "export function registerServerReference(action, id, name) { globalThis.actions ??= new Map(); globalThis.actions.set(id, {action, name}); }",
  };
  fs.mkdirSync(inputPath, { recursive: true });
  fs.writeFileSync(path.join(projectPath, "package.json"), '{"private":true}');
  for (const [name, contents] of Object.entries(sources))
    fs.writeFileSync(path.join(inputPath, name), contents);

  const distPath = path.join(projectPath, "dist");
  const clientOutput = path.join(distPath, "client");
  const serverOutput = path.join(distPath, "server");
  for (const output of [clientOutput, serverOutput]) {
    fs.mkdirSync(output, { recursive: true });
    fs.writeFileSync(
      path.join(output, "stats.json"),
      JSON.stringify({ existing: output }),
    );
    fs.writeFileSync(path.join(output, "keep.js"), "existing bundle");
  }
  const beforeOutputs = readFiles(distPath);
  const serverConfig: NonNullable<ConfigComplete["server"]> = {
    output: { path: serverOutput, filename: "[name].[contenthash:8].js" },
  };
  const config: ConfigComplete = {
    mode: "production",
    entry: [],
    output: { path: clientOutput, clean: true },
    sourceMaps: false,
    stats: true,
    persistentCaching: false,
    optimization: { minify: true },
  };
  if (scenario === "node") {
    config.target = "current node";
    // The CLI enables concatenation: membership must include modules inside
    // scope-hoisted Node chunk items, not only their enclosing module.
    config.optimization = {
      ...config.optimization,
      concatenateModules: true,
      removeUnusedExports: true,
      removeUnusedImports: true,
    };
    config.entry = [
      { name: "main", import: "./src/server-main.js" },
      { name: "admin", import: "./src/server-admin.js" },
    ];
  } else {
    config.server = serverConfig;
    if (scenario === "mixed") {
      // Names can collide across environments; these are separate endpoints.
      config.entry = [{ name: "alpha", import: "./src/client.js" }];
      serverConfig.entry = [
        { name: "alpha", import: "./src/server-main.js" },
        { name: "admin", import: "./src/server-admin.js" },
      ];
    } else if (scenario === "server-only") {
      // Server entries use the native resolver and may omit their extension.
      serverConfig.entry = "./src/server-main";
    } else {
      config.entry = [
        { name: "functions", import: "./src/function-client.js" },
      ];
      serverConfig.function = {
        clientProxy: "./src/transport.js",
        serverRegister: "./src/register.js",
      };
      if (withFunctionEntry) {
        serverConfig.entry = [
          { name: "alpha", import: "./src/server-main.js" },
          { name: "admin", import: "./src/server-admin.js" },
        ];
      }
    }
  }
  const originalConfig = JSON.stringify(config);
  const reportPath = path.join(projectPath, "report");
  const result = await analyze(
    { config, tracing: false },
    projectPath,
    projectPath,
    { outputPath: reportPath },
  );
  assert.equal(
    JSON.stringify(config),
    originalConfig,
    "Analysis changed server configuration",
  );
  assert.deepEqual(
    readFiles(distPath),
    beforeOutputs,
    "Analysis rewrote client/server bundles or stats",
  );
  assert(fs.existsSync(path.join(reportPath, "index.html")));
  assert.equal(
    result.entries.length,
    scenario === "mixed" || withFunctionEntry
      ? 3
      : scenario === "server-only"
        ? 1
        : 2,
  );
  assert.equal(
    new Set(result.entries.map((entry) => entry.path)).size,
    result.entries.length,
    "Entry data paths collide",
  );
  assert.deepEqual(
    JSON.parse(
      fs.readFileSync(path.join(reportPath, "data/entries.json"), "utf8"),
    ),
    result.entries,
  );
  assert(
    !Object.keys(readFiles(reportPath)).some((filename) =>
      filename.endsWith("stats.json"),
    ),
  );
  const graph = readArtifact<ModuleHeader>(
    path.join(reportPath, "data/modules.data"),
  );
  const reports = result.entries.map((entry) => {
    const artifact = readArtifact<AnalyzeHeader>(
      path.join(reportPath, "data", entry.path.slice(1), "analyze.data"),
    );
    const members = new Set(
      artifact.header.output_files.flatMap((_, index) =>
        readEdges(artifact.binary, artifact.header.output_file_modules, index),
      ),
    );
    assert.equal(
      artifact.header.module_index_hash,
      graph.header.module_index_hash,
    );
    assert(
      artifact.header.output_files.length > 0,
      `${entry.name} report is empty`,
    );
    assert(
      artifact.header.chunk_parts.some((part) => part.size > 0),
      `${entry.name} has no attributed bytes`,
    );
    for (const index of members)
      assert(index < graph.header.modules.length, "Invalid graph member index");
    assert(
      !sourcePaths(artifact.header).some((source) =>
        source.endsWith("/original-server.ts"),
      ),
      "Server source attribution no longer matches resolved JavaScript module identities",
    );
    assert(["client", "server"].includes(entry.environment));
    assert(
      artifact.header.route_entries.every(
        (root) =>
          root.entry_kind ===
          (entry.environment === "client" ? "client_bootstrap" : "server"),
      ),
      `Wrong graph-root environment for ${entry.name}`,
    );
    assert(
      artifact.header.output_files.every(
        (file) =>
          file.filename.startsWith("[client-fs]/") ===
          (entry.environment === "client"),
      ),
      `Wrong output environment for ${entry.name}`,
    );
    return {
      entry,
      artifact,
      scope: {
        roots: new Set(
          artifact.header.route_entries.map((root) => root.module_ident),
        ),
        members,
      },
    };
  });
  type Report = (typeof reports)[number];
  const findReport = (suffix: string, environment: "client" | "server") => {
    const matches = reports.filter(
      (report) =>
        report.entry.environment === environment &&
        report.artifact.header.route_entries.some((root) =>
          root.module_path.endsWith(suffix),
        ),
    );
    assert.equal(
      matches.length,
      1,
      `Expected one ${environment} entry rooted at ${suffix}`,
    );
    return matches[0];
  };
  const verify = (
    report: Report,
    root: string,
    included: string[],
    excluded: string[],
  ) => {
    assertSource(report.artifact.header, root);
    for (const suffix of included) {
      assertSource(report.artifact.header, suffix);
      assert(
        hasImportPath(graph, root, suffix, false, report.scope),
        `Missing scoped import chain from ${root} to ${suffix}`,
      );
    }
    for (const suffix of excluded)
      assert(
        !sourcePaths(report.artifact.header).some((source) =>
          source.endsWith(suffix),
        ),
        `${root} unexpectedly includes ${suffix}`,
      );
  };
  if (scenario !== "server-functions") {
    const main = findReport("/src/server-main.js", "server");
    verify(
      main,
      "/src/server-main.js",
      ["/src/shared.js", "/src/server-only.js", "/src/server-lazy.js"],
      ["/src/client-only.js", "/src/admin-only.js"],
    );
    assert(
      hasImportPath(
        graph,
        "/src/server-main.js",
        "/src/server-lazy.js",
        true,
        main.scope,
      ),
    );
    if (scenario !== "server-only") {
      const admin = findReport("/src/server-admin.js", "server");
      verify(
        admin,
        "/src/server-admin.js",
        ["/src/shared.js", "/src/admin-only.js"],
        [
          "/src/client-only.js",
          "/src/server-only.js",
          "/src/server-lazy.js",
          "/src/action-private.js",
        ],
      );
    }
  }
  if (scenario === "node" || scenario === "server-only") {
    assert(reports.every((report) => report.entry.environment === "server"));
  } else {
    const functionCase = scenario.startsWith("server-functions");
    const clientRoot = functionCase
      ? "/src/function-client.js"
      : "/src/client.js";
    const serverRoot = functionCase ? "/src/actions.js" : "/src/server-main.js";
    const client = findReport(clientRoot, "client");
    const server = findReport(serverRoot, "server");
    verify(
      client,
      clientRoot,
      [
        "/src/shared.js",
        ...(functionCase ? ["/src/transport.js"] : ["/src/client-only.js"]),
      ],
      [
        "/src/server-only.js",
        "/src/admin-only.js",
        "/src/action-private.js",
        "/src/register.js",
      ],
    );
    if (functionCase)
      verify(
        server,
        serverRoot,
        ["/src/shared.js", "/src/action-private.js", "/src/register.js"],
        ["/src/function-client.js", "/src/transport.js"],
      );
    if (withFunctionEntry) {
      assert.equal(
        server.entry.path,
        findReport("/src/server-main.js", "server").entry.path,
        "Server Functions must belong to the primary configured server entry",
      );
    }
    const sharedMembers = (report: Report) =>
      [...report.scope.members].filter((index) =>
        graph.header.modules[index].path.endsWith("/src/shared.js"),
      );
    const clientShared = sharedMembers(client);
    const serverShared = sharedMembers(server);
    assert(
      clientShared.length > 0 && serverShared.length > 0,
      "Shared source missing in one environment",
    );
    assert(
      clientShared.every((index) => !serverShared.includes(index)),
      "Client/server shared source identities were merged",
    );
    if (scenario === "mixed") {
      assert.equal(client.entry.name, "alpha");
      assert.equal(server.entry.name, "alpha");
    }
  }
  if (scenario === "mixed" || scenario === "node" || withFunctionEntry) {
    delete process.env.ANALYZE;
    await build({ config, tracing: false }, projectPath, projectPath);
    const statsAt = (output: string) =>
      JSON.parse(fs.readFileSync(path.join(output, "stats.json"), "utf8"));
    const topStats = statsAt(clientOutput);
    assert.deepEqual(
      Object.keys(topStats.entrypoints).sort(),
      scenario === "mixed"
        ? ["alpha"]
        : withFunctionEntry
          ? ["functions"]
          : ["admin", "main"],
    );
    if (scenario === "mixed" || withFunctionEntry)
      assert.deepEqual(Object.keys(statsAt(serverOutput).entrypoints).sort(), [
        "admin",
        "alpha",
      ]);
  }
}

async function main() {
  // This override is Pack-specific; domparser-rs must use its own native addon.
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { analyze } = await import("../commands/analyze");
  const { build } = await import("../commands/build");
  if (
    [
      "node",
      "mixed",
      "server-only",
      "server-functions",
      "server-functions-entry",
    ].includes(scenario)
  ) {
    await serverScenario(analyze, build);
    return;
  }
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
    optimization: {
      minify: true,
      ...(scenario === "single"
        ? {
            concatenateModules: true,
            removeUnusedExports: true,
            removeUnusedImports: true,
          }
        : {}),
    },
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
  assert(result.entries.every((entry) => entry.environment === "client"));
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
  if (scenario === "single") {
    const entry = result.entries.find((entry) => entry.name === "alpha")!;
    const artifact = readArtifact<AnalyzeHeader>(
      path.join(reportPath, "data", entry.path.slice(1), "analyze.data"),
    );
    const members = new Set(
      artifact.header.output_files.flatMap((_, index) =>
        readEdges(artifact.binary, artifact.header.output_file_modules, index),
      ),
    );
    const scope = {
      members,
      roots: new Set(
        artifact.header.route_entries.map((root) => root.module_ident),
      ),
    };
    for (const suffix of [
      "/src/alpha-only.js",
      "/src/compiled-widget.js",
      "/src/shared.js",
    ]) {
      assert(
        hasImportPath(graph, "/src/alpha.js", suffix, false, scope),
        `Scope-hoisted browser output is missing the actual member ${suffix}`,
      );
    }
  }
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
