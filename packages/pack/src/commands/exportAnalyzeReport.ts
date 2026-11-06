// Adapted from Next.js's MIT-licensed build/analyze/graph-dump.ts. This public
// format describes the graph without exposing the analyzer UI's binary layout.
import { readFile, realpath } from "node:fs/promises";
import path from "node:path";

export type AnalyzeExportEnvironment = "client" | "server";
export type AnalyzeExportCoverage = "exact" | "unsupported" | "not_a_chunk";

export interface AnalyzeExportOptions {
  environment?: AnalyzeExportEnvironment;
  /** Exact display name or /entry-N ID. Use an ID to disambiguate equal names. */
  entry?: string;
}

export interface AnalyzeExportMetaRecord {
  type: "meta";
  schema_version: 1;
  tool: "@utoo/pack";
  module_index_hash: string;
  entry_count: number;
  selected_entries: number;
}

export interface AnalyzeExportModuleRecord {
  type: "module";
  ident: string;
  path: string;
  dependencies: { sync: string[]; async: string[]; traced: string[] };
}

export interface AnalyzeExportRoot {
  id: string;
  module_ident: string;
  module_path: string;
  role: string;
  entry_kind?: string;
  client_references: Array<{
    module_ident: string;
    module_path: string;
    reference_kind: string;
  }>;
}

export interface AnalyzeExportEntryRecord {
  type: "entry";
  /** Stable within this report; join output, part and group records on this ID. */
  entry: string;
  name: string;
  environment: AnalyzeExportEnvironment;
  roots: AnalyzeExportRoot[] | null;
  coverage: { roots: "exact" | "unknown"; groups: "exact" | "unknown" };
}

export interface AnalyzeExportOutputRecord {
  type: "output";
  entry: string;
  filename: string;
  /** The output's environment, which can differ from its entry's environment. */
  environment: AnalyzeExportEnvironment;
  kind: "emitted" | "traced";
  modules: string[];
  async_loaders: string[];
  coverage: AnalyzeExportCoverage;
  size: number;
  compressed_size: number;
}

export interface AnalyzeExportPartRecord {
  type: "part";
  entry: string;
  /** Unique within the entry. Do not deduplicate by filename + source_path. */
  id: number;
  filename: string;
  source_path: string;
  size: number;
  compressed_size: number;
}

export interface AnalyzeExportGroupRecord {
  type: "group";
  entry: string;
  id: number;
  kind: string;
  trigger_module_ident: string | null;
  trigger_join: "none" | "joined";
  outputs: string[];
}

export type AnalyzeExportRecord =
  | AnalyzeExportMetaRecord
  | AnalyzeExportModuleRecord
  | AnalyzeExportEntryRecord
  | AnalyzeExportOutputRecord
  | AnalyzeExportPartRecord
  | AnalyzeExportGroupRecord;

type ObjectValue = Record<string, unknown>;
type Entry = {
  name: string;
  path: string;
  environment: AnalyzeExportEnvironment;
};
type EdgeRows = { row(index: number): number[] };
const UINT32_LIMIT = 0x1_0000_0000;

function object(value: unknown, label: string): ObjectValue {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Invalid analyzer ${label}`);
  }
  return value as ObjectValue;
}

function array(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`Invalid analyzer ${label}`);
  return value;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`Invalid analyzer ${label}`);
  return value;
}

function integer(value: unknown, limit: number): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value < limit
  );
}

function index(value: unknown, limit: number, label: string): number {
  if (!integer(value, limit)) {
    throw new Error(`Invalid analyzer ${label} index: ${String(value)}`);
  }
  return value;
}

function json(data: Buffer, label: string): unknown {
  try {
    return JSON.parse(data.toString("utf8"));
  } catch (error) {
    throw new Error(`Invalid analyzer JSON in ${label}`, { cause: error });
  }
}

/** Constrain every report-controlled file, including symlinks, to the report. */
async function reportFile(root: string, relative: string): Promise<Buffer> {
  const filename = await realpath(path.join(root, relative));
  const location = path.relative(root, filename);
  if (
    location === ".." ||
    location.startsWith(`..${path.sep}`) ||
    path.isAbsolute(location)
  ) {
    throw new Error(`Analyzer file escapes report directory: ${relative}`);
  }
  return readFile(filename);
}

async function dataFile(root: string, relative: string) {
  const data = await reportFile(root, relative);
  if (data.length < 4) {
    throw new Error(`Truncated analyzer header: ${relative}`);
  }
  const length = data.readUInt32BE(0);
  if (length === 0 || length > data.length - 4) {
    throw new Error(`Invalid analyzer header length: ${relative}`);
  }
  const header = object(json(data.subarray(4, 4 + length), relative), "header");
  if (header.schema_version !== 1) {
    throw new Error(
      `Unsupported analyzer schema ${String(header.schema_version)}: ${relative}`,
    );
  }
  return { header, binary: data.subarray(4 + length) };
}

function edges(
  binary: Buffer,
  value: unknown,
  rows: number,
  targets: number,
  label: string,
): EdgeRows {
  const ref = object(value, `${label} section`);
  const { offset, length } = ref;
  if (
    !integer(offset, binary.length + 1) ||
    !integer(length, binary.length + 1) ||
    length < 4 ||
    offset + length > binary.length
  ) {
    throw new Error(`Invalid analyzer ${label} section`);
  }
  const count = binary.readUInt32BE(offset);
  if (count !== rows || length < 4 + 4 * count) {
    throw new Error(`Invalid analyzer ${label} row count`);
  }
  const dataStart = offset + 4 + 4 * count;
  let previous = 0;
  for (let row = 0; row < count; row++) {
    const end = binary.readUInt32BE(offset + 4 + row * 4);
    if (end < previous || 4 + 4 * count + 4 * end > length) {
      throw new Error(`Invalid analyzer ${label} offsets`);
    }
    previous = end;
  }
  if (4 + 4 * count + 4 * previous !== length) {
    throw new Error(`Invalid analyzer ${label} section length`);
  }
  for (let target = 0; target < previous; target++) {
    index(binary.readUInt32BE(dataStart + target * 4), targets, label);
  }
  return {
    row(row) {
      const start = row === 0 ? 0 : binary.readUInt32BE(offset + row * 4);
      const end = binary.readUInt32BE(offset + 4 + row * 4);
      const result: number[] = [];
      for (let target = start; target < end; target++) {
        result.push(binary.readUInt32BE(dataStart + target * 4));
      }
      return result;
    },
  };
}

async function readModules(root: string) {
  const { header, binary } = await dataFile(root, "data/modules.data");
  const byIdent = new Map<string, string>();
  const modules = array(header.modules, "modules").map((value) => {
    const module = object(value, "module");
    const ident = string(module.ident, "module identity");
    const modulePath = string(module.path, "module path");
    if (!ident || byIdent.has(ident)) {
      throw new Error("Invalid or duplicated analyzer module identity");
    }
    byIdent.set(ident, modulePath);
    return { ident, path: modulePath };
  });
  const hash = string(header.module_index_hash, "module-index fingerprint");
  if (!hash) throw new Error("Missing analyzer module-index fingerprint");
  return {
    modules,
    byIdent,
    hash,
    edges: {
      sync: edges(
        binary,
        header.module_dependencies,
        modules.length,
        modules.length,
        "module dependencies",
      ),
      async: edges(
        binary,
        header.async_module_dependencies,
        modules.length,
        modules.length,
        "async module dependencies",
      ),
      traced: edges(
        binary,
        header.traced_module_dependencies,
        modules.length,
        modules.length,
        "traced module dependencies",
      ),
    },
  };
}

type ModuleData = Awaited<ReturnType<typeof readModules>>;

function sourcePaths(value: unknown): string[] {
  const values = array(value, "sources");
  const sources = values.map((value) => {
    const source = object(value, "source");
    return {
      path: string(source.path, "source path"),
      parent:
        source.parent_source_index === null
          ? null
          : index(source.parent_source_index, values.length, "source parent"),
    };
  });
  const paths: string[] = Array(sources.length);
  for (let start = 0; start < sources.length; start++) {
    const chain: number[] = [];
    const visiting = new Set<number>();
    let cursor: number | null = start;
    while (cursor !== null && paths[cursor] === undefined) {
      if (visiting.has(cursor)) throw new Error("Cyclic analyzer source tree");
      visiting.add(cursor);
      chain.push(cursor);
      cursor = sources[cursor].parent;
    }
    for (let i = chain.length - 1; i >= 0; i--) {
      const current = chain[i];
      const { parent, path: sourcePath } = sources[current];
      paths[current] = (parent === null ? "" : paths[parent]) + sourcePath;
    }
  }
  return paths;
}

function moduleReference(value: unknown, modules: ModuleData) {
  const reference = object(value, "module reference");
  const module_ident = string(reference.module_ident, "root module identity");
  const module_path = string(reference.module_path, "root module path");
  if (
    !modules.byIdent.has(module_ident) ||
    modules.byIdent.get(module_ident) !== module_path
  ) {
    throw new Error(
      `Analyzer root module does not match module index: ${module_ident}`,
    );
  }
  return { module_ident, module_path };
}

function readRoots(
  value: unknown,
  modules: ModuleData,
): AnalyzeExportRoot[] | null {
  if (value === undefined) return null;
  const ids = new Set<string>();
  return array(value, "entry roots").map((value) => {
    const root = object(value, "entry root");
    const id = string(root.route_entry_id, "root identity");
    if (!id || ids.has(id))
      throw new Error("Invalid or duplicated analyzer root identity");
    ids.add(id);
    return {
      id,
      ...moduleReference(root, modules),
      role: string(root.role, "root role"),
      ...(root.entry_kind === undefined
        ? {}
        : { entry_kind: string(root.entry_kind, "entry kind") }),
      client_references: array(
        root.client_references === undefined ? [] : root.client_references,
        "client references",
      ).map((value) => {
        const reference = object(value, "client reference");
        return {
          ...moduleReference(reference, modules),
          reference_kind: string(
            reference.reference_kind,
            "client reference kind",
          ),
        };
      }),
    };
  });
}

function readGroups(
  value: unknown,
  modules: ModuleData,
  outputs: string[],
): Omit<AnalyzeExportGroupRecord, "type" | "entry">[] | null {
  if (value === undefined) return null;
  const ids = new Set<number>();
  return array(value, "chunk groups").map((value) => {
    const group = object(value, "chunk group");
    const id = index(group.id, UINT32_LIMIT, "group");
    if (ids.has(id))
      throw new Error("Duplicated analyzer chunk group identity");
    ids.add(id);
    const trigger = group.trigger_module_index;
    return {
      id,
      kind: string(group.kind, "chunk group kind"),
      trigger_module_ident:
        trigger === undefined
          ? null
          : modules.modules[
              index(trigger, modules.modules.length, "group trigger")
            ].ident,
      trigger_join: trigger === undefined ? "none" : "joined",
      outputs: array(group.output_file_indices, "chunk group outputs").map(
        (value) => outputs[index(value, outputs.length, "group output")],
      ),
    };
  });
}

async function readEntry(root: string, entry: Entry, modules: ModuleData) {
  const { header, binary } = await dataFile(
    root,
    `data${entry.path}/analyze.data`,
  );
  if (header.module_index_hash !== modules.hash) {
    throw new Error(
      `Analyzer module-index fingerprint mismatch: ${entry.path}`,
    );
  }
  const paths = sourcePaths(header.sources);
  const seen = new Set<string>();
  const outputs = array(header.output_files, "outputs").map((value) => {
    const filename = string(
      object(value, "output").filename,
      "output filename",
    );
    if (seen.has(filename))
      throw new Error(`Duplicated analyzer output filename: ${filename}`);
    seen.add(filename);
    return filename;
  });
  const totals = outputs.map(() => ({ size: 0, compressed_size: 0 }));
  const parts = array(header.chunk_parts, "parts").map((value, id) => {
    const part = object(value, "part");
    const output = index(part.output_file_index, outputs.length, "part output");
    const source = index(part.source_index, paths.length, "part source");
    const size = index(part.size, UINT32_LIMIT, "part size");
    const compressed_size = index(
      part.compressed_size,
      UINT32_LIMIT,
      "compressed part size",
    );
    totals[output].size += size;
    totals[output].compressed_size += compressed_size;
    if (
      !Number.isSafeInteger(totals[output].size) ||
      !Number.isSafeInteger(totals[output].compressed_size)
    ) {
      throw new Error(
        "Analyzer output size exceeds the supported integer range",
      );
    }
    return {
      id,
      filename: outputs[output],
      source_path: paths[source],
      size,
      compressed_size,
    };
  });
  const coverage = array(
    header.output_file_module_coverage,
    "output coverage",
  ).map((value): AnalyzeExportCoverage => {
    if (
      value !== "exact" &&
      value !== "unsupported" &&
      value !== "not_a_chunk"
    ) {
      throw new Error("Invalid analyzer output coverage");
    }
    return value;
  });
  if (coverage.length !== outputs.length)
    throw new Error("Invalid analyzer output coverage length");
  return {
    outputs,
    parts,
    totals,
    coverage,
    membership: edges(
      binary,
      header.output_file_modules,
      outputs.length,
      modules.modules.length,
      "output modules",
    ),
    asyncLoaders: edges(
      binary,
      header.output_file_async_loaders,
      outputs.length,
      modules.modules.length,
      "output async loaders",
    ),
    roots: readRoots(header.route_entries, modules),
    groups: readGroups(header.chunk_groups, modules, outputs),
  };
}

async function readEntries(root: string): Promise<Entry[]> {
  const marker = object(
    json(await reportFile(root, "utoo-analyzer.json"), "report marker"),
    "report marker",
  );
  if (marker.tool !== "@utoo/pack-analyzer" || marker.version !== 1) {
    throw new Error(
      "Unsupported Utoopack analyzer report format; capture a new report with up analyze.",
    );
  }
  const entries = array(
    json(await reportFile(root, "data/entries.json"), "entries.json"),
    "entries",
  );
  const paths = new Set<string>();
  return entries.map((value) => {
    const entry = object(value, "entry");
    const entryPath = string(entry.path, "entry path");
    if (!/^\/entry-\d+$/.test(entryPath) || paths.has(entryPath)) {
      throw new Error(
        `Invalid or duplicated analyzer entry path: ${entryPath}`,
      );
    }
    paths.add(entryPath);
    // Early native reports contained only client entries, without the label.
    const environment =
      entry.environment === undefined ? "client" : entry.environment;
    if (environment !== "client" && environment !== "server") {
      throw new Error("Invalid analyzer entry environment");
    }
    return {
      name: string(entry.name, "entry name"),
      path: entryPath,
      environment,
    };
  });
}

/**
 * Export saved build evidence without loading native bindings, compiling or serving.
 * Keeps the complete module graph and only one entry's payload in memory. Filters
 * select entry records, not module graph nodes, so import chains remain intact.
 * A later invalid entry can reject after earlier records were yielded: consumers
 * must discard partial output on failure. Compressed sizes are estimates.
 */
export async function* exportAnalyzeReport(
  reportPath: string,
  options: AnalyzeExportOptions = {},
): AsyncGenerator<AnalyzeExportRecord> {
  if (
    options.environment !== undefined &&
    options.environment !== "client" &&
    options.environment !== "server"
  ) {
    throw new Error("Analyzer environment must be client or server.");
  }
  if (options.entry !== undefined && typeof options.entry !== "string") {
    throw new Error("Analyzer entry filter must be a string.");
  }
  const root = await realpath(reportPath);
  const entries = await readEntries(root);
  let selected = entries.filter(
    (entry) =>
      options.environment === undefined ||
      entry.environment === options.environment,
  );
  if (options.entry !== undefined) {
    const filter = options.entry;
    selected = selected.filter((entry) =>
      /^\/entry-\d+$/.test(filter)
        ? entry.path === filter
        : entry.name === filter,
    );
    if (selected.length > 1) {
      throw new Error(
        `Ambiguous analyzer entry ${filter}; select an environment or /entry-N ID.`,
      );
    }
  }
  if (
    selected.length === 0 &&
    (options.entry !== undefined || options.environment !== undefined)
  ) {
    throw new Error("No analyzer entries match the requested filters.");
  }
  const data = await readModules(root);
  yield {
    type: "meta",
    schema_version: 1,
    tool: "@utoo/pack",
    module_index_hash: data.hash,
    entry_count: entries.length,
    selected_entries: selected.length,
  };
  for (const [id, module] of data.modules.entries()) {
    yield {
      type: "module",
      ...module,
      dependencies: {
        sync: data.edges.sync.row(id).map((index) => data.modules[index].ident),
        async: data.edges.async
          .row(id)
          .map((index) => data.modules[index].ident),
        traced: data.edges.traced
          .row(id)
          .map((index) => data.modules[index].ident),
      },
    };
  }
  for (const entry of selected) {
    const report = await readEntry(root, entry, data);
    yield {
      type: "entry",
      entry: entry.path,
      name: entry.name,
      environment: entry.environment,
      roots: report.roots,
      coverage: {
        roots: report.roots === null ? "unknown" : "exact",
        groups: report.groups === null ? "unknown" : "exact",
      },
    };
    for (const [id, filename] of report.outputs.entries()) {
      yield {
        type: "output",
        entry: entry.path,
        filename,
        environment: filename.startsWith("[client-fs]/") ? "client" : "server",
        kind: filename.startsWith("[project]/") ? "traced" : "emitted",
        modules: report.membership
          .row(id)
          .map((index) => data.modules[index].ident)
          .sort(),
        async_loaders: report.asyncLoaders
          .row(id)
          .map((index) => data.modules[index].ident)
          .sort(),
        coverage: report.coverage[id],
        ...report.totals[id],
      };
    }
    for (const part of report.parts)
      yield { type: "part", entry: entry.path, ...part };
    for (const group of report.groups ?? [])
      yield { type: "group", entry: entry.path, ...group };
  }
}
