import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwind from "@tailwindcss/postcss";
import { build } from "esbuild";
import postcss from "postcss";

const workspace = path.dirname(fileURLToPath(import.meta.url));
const repository = path.join(workspace, "../..");
const upstream = path.join(repository, "next.js/apps/bundle-analyzer");
const output = path.join(workspace, "../pack/analyzer");
const upstreamStyles = path.join(upstream, "app/globals.css");

await mkdir(output, { recursive: true });

function replaceUpstream(source, file, before, after) {
  if (!source.includes(before)) {
    throw new Error(
      `Check the Pack adapter for ${path.relative(upstream, file)} after updating the upstream analyzer.`,
    );
  }
  return source.replace(before, after);
}

// The native format includes exact roots but this revision of the upstream
// browser parser does not expose them. Add the accessor without reparsing the
// binary header or carrying a duplicate copy of the upstream parser.
const rootAccessorAnchor = "  sourceRoots(): SourceIndex[] {";
const standaloneEdits = {
  "lib/analyze-data.ts": [[
    rootAccessorAnchor,
    `  packEntryModuleIdents(): string[] {
    const header = this.analyzeHeader as AnalyzeDataHeader & {
      route_entries?: { module_ident: string }[]
    }
    return header.route_entries?.map(({ module_ident }) => module_ident) ?? []
  }

${rootAccessorAnchor}`,
  ]],
  "lib/treemap-layout.ts": [["All Route Modules", "All Entry Modules"]],
  "components/sidebar.tsx": [["All Route Modules", "All Entry Modules"]],
  "components/import-chain.tsx": [
    ["Current route only", "Current entry only"],
    ["current route's bundle", "current entry's bundle"],
    // Pack names its Node module layer `server`; Next uses SSR/RSC/API layers.
    ["/ssr|rsc|route|api/.test(layer)", "/ssr|rsc|route|api|server/.test(layer)"],
    ["Pages Router (${layer})", "Module layer (${layer})"],
  ],
};

const standaloneAdapters = {
  name: "pack-analyzer-adapters",
  setup(context) {
    context.onLoad(
      { filter: /[/\\](analyze-data|treemap-layout|sidebar|import-chain)\.tsx?$/ },
      async (args) => {
        const relative = path.relative(upstream, args.path).split(path.sep).join("/");
        const edits = standaloneEdits[relative];
        if (!edits) return;
        let contents = await readFile(args.path, "utf8");
        for (const [before, after] of edits) {
          contents = replaceUpstream(contents, args.path, before, after);
        }
        return {
          contents,
          loader: path.extname(args.path).slice(1),
          resolveDir: path.dirname(args.path),
        };
      },
    );
  },
};

async function bundleStyles() {
  const source = await readFile(upstreamStyles, "utf8");
  const css = replaceUpstream(
    source,
    upstreamStyles,
    "@import 'tailwindcss';",
    `@import 'tailwindcss' source(none);
@source "./src";
@source "../../next.js/apps/bundle-analyzer/components";`,
  );
  const result = await postcss([
    tailwind({ base: workspace, optimize: true }),
  ]).process(css, { from: path.join(workspace, "analyzer.css") });
  await writeFile(path.join(output, "analyzer.css"), result.css);
}

const [bundle] = await Promise.all([
  build({
    absWorkingDir: workspace,
    entryPoints: ["src/main.tsx"],
    outfile: path.join(output, "analyzer.js"),
    bundle: true,
    minify: true,
    jsx: "automatic",
    platform: "browser",
    target: "es2022",
    format: "esm",
    metafile: true,
    alias: {
      "@": upstream,
      "next/link": path.join(workspace, "src/static-link.tsx"),
    },
    nodePaths: [
      path.join(workspace, "node_modules"),
      path.join(repository, "node_modules"),
    ],
    plugins: [standaloneAdapters],
    banner: { js: "/* Turbopack Bundle Analyzer UI. See LICENSE.next.js.md. */" },
  }),
  bundleStyles(),
  copyFile(path.join(workspace, "index.html"), path.join(output, "index.html")),
  copyFile(
    path.join(repository, "next.js/license.md"),
    path.join(output, "LICENSE.next.js.md"),
  ),
]);

// Include full license text for the dependencies present in the generated UI.
const packageDirectories = new Set();
function includeLicense(filename) {
  const parts = path.resolve(workspace, filename).split(path.sep);
  const index = parts.lastIndexOf("node_modules");
  if (index < 0) return;
  const packageEnd = index + (parts[index + 1].startsWith("@") ? 3 : 2);
  packageDirectories.add(parts.slice(0, packageEnd).join(path.sep));
}

for (const { inputs } of Object.values(bundle.metafile.outputs)) {
  for (const [filename, { bytesInOutput }] of Object.entries(inputs)) {
    if (bytesInOutput > 0) includeLicense(filename);
  }
}
for (const name of ["tailwindcss", "tw-animate-css"]) {
  includeLicense(fileURLToPath(import.meta.resolve(name)));
}

const notices = await Promise.all(
  [...packageDirectories].map(async (directory) => {
    const { name, version, license } = JSON.parse(
      await readFile(path.join(directory, "package.json"), "utf8"),
    );
    const files = (await readdir(directory)).filter((file) =>
      /^(licen[cs]e|notice|copying)([.-].*)?$/i.test(file),
    );
    const contents = await Promise.all(
      files.sort().map((file) => readFile(path.join(directory, file), "utf8")),
    );
    return `${name}@${version} (${license ?? "see notice"})\n\n${contents.join("\n\n")}`;
  }),
);
await writeFile(
  path.join(output, "THIRD-PARTY-NOTICES.txt"),
  `${[...new Set(notices)].sort().join("\n\n------\n\n")}\n`,
);
