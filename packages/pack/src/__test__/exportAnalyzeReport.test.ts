import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type AnalyzeExportOptions,
  type AnalyzeExportRecord,
  exportAnalyzeReport,
} from "../commands/exportAnalyzeReport";

// Report export must work on a machine without a native addon or project.
vi.mock("../binding", () => {
  throw new Error("Report export loaded the native addon");
});
vi.mock("../config", () => {
  throw new Error("Report export loaded project configuration");
});
vi.mock("../commands/dev", () => {
  throw new Error("Report export loaded the dev server");
});
vi.mock("../commands/serveAnalyzeReport", () => {
  throw new Error("Report export loaded the report server");
});

const moduleHash = "0123456789abcdef";
const modules = [
  { ident: "[project]/src/index.js [client]", path: "[project]/src/index.js" },
  {
    ident: "[project]/src/shared.js [client]",
    path: "[project]/src/shared.js",
  },
  { ident: "[project]/src/lazy.js [client]", path: "[project]/src/lazy.js" },
  {
    ident: "[project]/src/server.js [server]",
    path: "[project]/src/server.js",
  },
  {
    ident: "[project]/vendor/percent%FF.cjs [trace]",
    path: "[project]/vendor/percent%FF.cjs",
  },
];
const entries = [
  { name: "app", path: "/entry-0", environment: "client" },
  { name: "app", path: "/entry-1", environment: "server" },
];

let fixture: string;
let report: string;

// Write the native u32 big-endian edge format independently from the decoder.
function artifact(
  header: Record<string, unknown>,
  edgeRows: Record<string, number[][]>,
): Buffer {
  const sections: Buffer[] = [];
  const refs: Record<string, { offset: number; length: number }> = {};
  let offset = 0;
  for (const [name, rows] of Object.entries(edgeRows)) {
    const values = rows.flat();
    const section = Buffer.alloc(4 * (1 + rows.length + values.length));
    section.writeUInt32BE(rows.length, 0);
    let end = 0;
    rows.forEach((row, index) => {
      end += row.length;
      section.writeUInt32BE(end, 4 * (index + 1));
    });
    values.forEach((value, index) =>
      section.writeUInt32BE(value, 4 * (1 + rows.length + index)),
    );
    refs[name] = { offset, length: section.length };
    sections.push(section);
    offset += section.length;
  }
  const json = Buffer.from(JSON.stringify({ ...header, ...refs }));
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32BE(json.length);
  return Buffer.concat([prefix, json, ...sections]);
}

function writeJson(filename: string, value: unknown) {
  fs.writeFileSync(path.join(report, filename), JSON.stringify(value));
}

function rewriteHeader(
  filename: string,
  mutate: (header: Record<string, any>) => void,
) {
  const file = path.join(report, filename);
  const bytes = fs.readFileSync(file);
  const end = 4 + bytes.readUInt32BE(0);
  const header = JSON.parse(bytes.subarray(4, end).toString());
  mutate(header);
  const json = Buffer.from(JSON.stringify(header));
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32BE(json.length);
  fs.writeFileSync(file, Buffer.concat([prefix, json, bytes.subarray(end)]));
}

function writeReport() {
  fs.mkdirSync(path.join(report, "data/entry-0"), { recursive: true });
  fs.mkdirSync(path.join(report, "data/entry-1"), { recursive: true });
  writeJson("utoo-analyzer.json", { tool: "@utoo/pack-analyzer", version: 1 });
  writeJson("data/entries.json", entries);
  fs.writeFileSync(
    path.join(report, "data/modules.data"),
    artifact(
      { schema_version: 1, module_index_hash: moduleHash, modules },
      {
        module_dependencies: [[1], [], [], [], []],
        async_module_dependencies: [[2], [], [], [], []],
        traced_module_dependencies: [[], [], [], [4], []],
        module_dependents: [[], [0], [], [], []],
        async_module_dependents: [[], [], [0], [], []],
        traced_module_dependents: [[], [], [], [], [3]],
      },
    ),
  );
  for (const [index, entry] of entries.entries()) {
    const client = index === 0;
    const root = modules[client ? 0 : 3];
    fs.writeFileSync(
      path.join(report, "data", entry.path.slice(1), "analyze.data"),
      artifact(
        {
          schema_version: 1,
          module_index_hash: moduleHash,
          sources: [
            { parent_source_index: null, path: "[project]/" },
            ...(client
              ? ["src/index.js", "src/shared.js", "src/lazy.js"]
              : ["src/server.js", "vendor/percent%FF.cjs"]
            ).map((name) => ({ parent_source_index: 0, path: name })),
          ],
          source_roots: [0],
          chunk_parts: client
            ? [
                {
                  source_index: 1,
                  output_file_index: 0,
                  size: 80,
                  compressed_size: 40,
                },
                {
                  source_index: 2,
                  output_file_index: 0,
                  size: 20,
                  compressed_size: 8,
                },
                {
                  source_index: 3,
                  output_file_index: 1,
                  size: 30,
                  compressed_size: 12,
                },
                {
                  source_index: 1,
                  output_file_index: 0,
                  size: 20,
                  compressed_size: 10,
                },
              ]
            : [
                {
                  source_index: 1,
                  output_file_index: 0,
                  size: 90,
                  compressed_size: 30,
                },
                {
                  source_index: 2,
                  output_file_index: 1,
                  size: 80,
                  compressed_size: 40,
                },
              ],
          output_files: (client
            ? ["[client-fs]/main.js", "[client-fs]/lazy.js"]
            : ["[output]/server.js", "[project]/vendor/percent%FF.cjs"]
          ).map((filename) => ({ filename })),
          output_file_module_coverage: client
            ? ["exact", "exact"]
            : ["exact", "not_a_chunk"],
          route_entries: [
            {
              route_entry_id: `${entry.path}|primary||${root.ident}`,
              module_ident: root.ident,
              module_path: root.path,
              role: "primary",
              entry_kind: client ? "client_bootstrap" : "server",
            },
          ],
          chunk_group_coverage: "exact",
          chunk_groups: client
            ? [
                {
                  id: 0,
                  kind: "bootstrap",
                  trigger_module_index: 0,
                  output_file_indices: [0],
                },
                {
                  id: 1,
                  kind: "async",
                  trigger_module_index: 2,
                  output_file_indices: [1],
                },
              ]
            : [
                {
                  id: 0,
                  kind: "server",
                  trigger_module_index: 3,
                  output_file_indices: [0],
                },
              ],
        },
        {
          output_file_modules: client ? [[0, 1], [2]] : [[3], []],
          output_file_async_loaders: client ? [[2], []] : [[], []],
          output_file_chunk_parts: client ? [[0, 1, 3], [2]] : [[0], [1]],
          source_chunk_parts: client ? [[], [0, 3], [1], [2]] : [[], [0], [1]],
          source_children: client ? [[1, 2, 3], [], [], []] : [[1, 2], [], []],
        },
      ),
    );
  }
}

beforeEach(() => {
  fixture = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-analyzer-export-"));
  report = path.join(fixture, "report");
  writeReport();
});

afterEach(() => {
  fs.rmSync(fixture, { recursive: true, force: true });
});

async function collect(options?: AnalyzeExportOptions) {
  const records: AnalyzeExportRecord[] = [];
  for await (const record of exportAnalyzeReport(report, options))
    records.push(record);
  return records;
}

describe("saved analyzer report export", () => {
  it("exports deterministic JSONL with exact graph joins and additive sizes without native imports", async () => {
    const records = await collect();
    const jsonl = records.map((record) => JSON.stringify(record)).join("\n");
    expect(
      (await collect()).map((record) => JSON.stringify(record)).join("\n"),
    ).toBe(jsonl);
    expect(jsonl.split("\n").map((line) => JSON.parse(line))).toEqual(records);
    expect(records[0]).toEqual({
      type: "meta",
      schema_version: 1,
      tool: "@utoo/pack",
      module_index_hash: moduleHash,
      entry_count: 2,
      selected_entries: 2,
    });
    const graph = records.filter((record) => record.type === "module");
    expect(
      graph.map((record) => ({ ident: record.ident, path: record.path })),
    ).toEqual(modules);
    expect(graph[0].dependencies).toEqual({
      sync: [modules[1].ident],
      async: [modules[2].ident],
      traced: [],
    });
    expect(graph[3].dependencies).toEqual({
      sync: [],
      async: [],
      traced: [modules[4].ident],
    });
    const exportedEntries = records.filter((record) => record.type === "entry");
    expect(
      exportedEntries.map((record) => ({
        name: record.name,
        path: record.entry,
        environment: record.environment,
      })),
    ).toEqual(entries);
    expect(
      exportedEntries.every(
        (record) =>
          record.coverage.roots === "exact" &&
          record.coverage.groups === "exact",
      ),
    ).toBe(true);
    expect(exportedEntries[0].roots).toEqual([
      {
        id: `/entry-0|primary||${modules[0].ident}`,
        module_ident: modules[0].ident,
        module_path: modules[0].path,
        role: "primary",
        entry_kind: "client_bootstrap",
        client_references: [],
      },
    ]);
    const outputs = records.filter((record) => record.type === "output");
    expect(outputs).toEqual([
      {
        type: "output",
        entry: "/entry-0",
        filename: "[client-fs]/main.js",
        environment: "client",
        kind: "emitted",
        modules: [modules[0].ident, modules[1].ident],
        async_loaders: [modules[2].ident],
        coverage: "exact",
        size: 120,
        compressed_size: 58,
      },
      {
        type: "output",
        entry: "/entry-0",
        filename: "[client-fs]/lazy.js",
        environment: "client",
        kind: "emitted",
        modules: [modules[2].ident],
        async_loaders: [],
        coverage: "exact",
        size: 30,
        compressed_size: 12,
      },
      {
        type: "output",
        entry: "/entry-1",
        filename: "[output]/server.js",
        environment: "server",
        kind: "emitted",
        modules: [modules[3].ident],
        async_loaders: [],
        coverage: "exact",
        size: 90,
        compressed_size: 30,
      },
      {
        type: "output",
        entry: "/entry-1",
        filename: "[project]/vendor/percent%FF.cjs",
        environment: "server",
        kind: "traced",
        modules: [],
        async_loaders: [],
        coverage: "not_a_chunk",
        size: 80,
        compressed_size: 40,
      },
    ]);
    const parts = records.filter((record) => record.type === "part");
    for (const output of outputs) {
      const joined = parts.filter(
        (part) =>
          part.entry === output.entry && part.filename === output.filename,
      );
      expect(joined.reduce((sum, part) => sum + part.size, 0)).toBe(
        output.size,
      );
      expect(joined.reduce((sum, part) => sum + part.compressed_size, 0)).toBe(
        output.compressed_size,
      );
    }
    expect(
      parts
        .filter(
          (part) =>
            part.entry === "/entry-0" && part.source_path === modules[0].path,
        )
        .map((part) => part.id),
    ).toEqual([0, 3]);
    expect(
      parts.find((part) => part.entry === "/entry-1" && part.id === 1)
        ?.source_path,
    ).toBe(modules[4].path);
    expect(records.filter((record) => record.type === "group")).toEqual([
      {
        type: "group",
        entry: "/entry-0",
        id: 0,
        kind: "bootstrap",
        trigger_module_ident: modules[0].ident,
        trigger_join: "joined",
        outputs: ["[client-fs]/main.js"],
      },
      {
        type: "group",
        entry: "/entry-0",
        id: 1,
        kind: "async",
        trigger_module_ident: modules[2].ident,
        trigger_join: "joined",
        outputs: ["[client-fs]/lazy.js"],
      },
      {
        type: "group",
        entry: "/entry-1",
        id: 0,
        kind: "server",
        trigger_module_ident: modules[3].ident,
        trigger_join: "joined",
        outputs: ["[output]/server.js"],
      },
    ]);
  });

  it.each([
    [{ environment: "client" }, "/entry-0"],
    [{ environment: "server", entry: "app" }, "/entry-1"],
    [{ entry: "/entry-1" }, "/entry-1"],
  ] as const)(
    "filters entry payloads with %j while retaining the whole import graph",
    async (options, selected) => {
      const records = await collect(options);
      expect(records[0]).toMatchObject({ entry_count: 2, selected_entries: 1 });
      expect(
        records
          .filter((record) => record.type === "module")
          .map((record) => record.ident),
      ).toEqual(modules.map((module) => module.ident));
      const scoped = records.filter((record) => "entry" in record);
      expect(scoped.length).toBeGreaterThan(0);
      expect(scoped.every((record) => record.entry === selected)).toBe(true);
    },
  );

  it("rejects ambiguous display names and nonexistent or contradictory selectors", async () => {
    await expect(collect({ entry: "app" })).rejects.toThrow(/ambiguous/i);
    await expect(collect({ entry: "missing" })).rejects.toThrow(
      /no.*entries.*match/i,
    );
    await expect(
      collect({ entry: "/entry-1", environment: "client" }),
    ).rejects.toThrow(/no.*entries.*match/i);
  });

  it("marks absent graph provenance unknown without inventing roots or groups", async () => {
    rewriteHeader("data/entry-0/analyze.data", (header) => {
      delete header.route_entries;
      delete header.chunk_groups;
      header.output_file_module_coverage[0] = "unsupported";
    });
    const records = await collect({ entry: "/entry-0" });
    expect(records.find((record) => record.type === "entry")).toMatchObject({
      roots: null,
      coverage: { roots: "unknown", groups: "unknown" },
    });
    expect(records.some((record) => record.type === "group")).toBe(false);
    expect(records.find((record) => record.type === "output")).toMatchObject({
      coverage: "unsupported",
    });
  });

  it.each(["unknown", "legacy"])(
    "preserves known groups with %s completeness",
    async (coverage) => {
      const before = await collect({ entry: "/entry-0" });
      const knownGroups = before.filter((record) => record.type === "group");
      expect(knownGroups).toHaveLength(2);
      rewriteHeader("data/entry-0/analyze.data", (header) => {
        if (coverage === "legacy") delete header.chunk_group_coverage;
        else header.chunk_group_coverage = coverage;
      });
      const records = await collect({ entry: "/entry-0" });
      expect(records.find((record) => record.type === "entry")).toMatchObject({
        coverage: { roots: "exact", groups: "unknown" },
      });
      expect(records.filter((record) => record.type === "group")).toEqual(
        knownGroups,
      );
    },
  );

  it.each(["partial", null, 0, {}])(
    "rejects invalid group completeness %j",
    async (coverage) => {
      rewriteHeader("data/entry-0/analyze.data", (header) => {
        header.chunk_group_coverage = coverage;
      });
      await expect(collect()).rejects.toThrow(/chunk group coverage/i);
    },
  );

  it("uses each output's actual environment independently from its entry", async () => {
    rewriteHeader("data/entry-1/analyze.data", (header) => {
      header.output_files[0].filename = "[client-fs]/server-reference.js";
    });
    const records = await collect({ entry: "/entry-1" });
    expect(records.find((record) => record.type === "entry")).toMatchObject({
      environment: "server",
    });
    expect(
      records
        .filter((record) => record.type === "output")
        .map((record) => record.environment),
    ).toEqual(["client", "server"]);
  });

  it.each([
    [
      "report marker",
      () =>
        writeJson("utoo-analyzer.json", { tool: "another-tool", version: 1 }),
    ],
    [
      "entry path traversal",
      () =>
        writeJson("data/entries.json", [
          { ...entries[0], path: "/entry-0/../../outside" },
        ]),
    ],
    [
      "duplicate entry paths",
      () => writeJson("data/entries.json", [entries[0], entries[0]]),
    ],
    [
      "schema version",
      () =>
        rewriteHeader("data/modules.data", (header) => {
          header.schema_version = 2;
        }),
    ],
    [
      "module fingerprint",
      () =>
        rewriteHeader("data/entry-0/analyze.data", (header) => {
          header.module_index_hash = "other-report";
        }),
    ],
    [
      "edge section",
      () =>
        rewriteHeader("data/modules.data", (header) => {
          header.module_dependencies.offset = 0xffff_ffff;
        }),
    ],
    [
      "output membership target",
      () =>
        mutateBinary(
          "data/entry-0/analyze.data",
          "output_file_modules",
          (section) => {
            section.writeUInt32BE(modules.length, 12);
          },
        ),
    ],
    [
      "edge offsets",
      () =>
        mutateBinary("data/modules.data", "module_dependencies", (section) => {
          section.writeUInt32BE(0xffff_ffff, 4);
        }),
    ],
    [
      "source cycle",
      () =>
        rewriteHeader("data/entry-0/analyze.data", (header) => {
          header.sources[0].parent_source_index = 1;
        }),
    ],
    [
      "part source index",
      () =>
        rewriteHeader("data/entry-0/analyze.data", (header) => {
          header.chunk_parts[0].source_index = 999;
        }),
    ],
    [
      "root graph identity",
      () =>
        rewriteHeader("data/entry-0/analyze.data", (header) => {
          header.route_entries[0].module_path = "different-path";
        }),
    ],
    [
      "group output index",
      () =>
        rewriteHeader("data/entry-0/analyze.data", (header) => {
          header.chunk_groups[0].output_file_indices = [999];
        }),
    ],
  ])("rejects malformed %s", async (_name, corrupt) => {
    corrupt();
    await expect(collect()).rejects.toThrow(/analyzer|report/i);
  });

  it("rejects truncated binary data and header lengths before decoding edges", async () => {
    const filename = path.join(report, "data/modules.data");
    const original = fs.readFileSync(filename);
    fs.writeFileSync(filename, original.subarray(0, 3));
    await expect(collect()).rejects.toThrow(/truncated.*header/i);
    const headerEnd = 4 + original.readUInt32BE(0);
    const header = JSON.parse(original.subarray(4, headerEnd).toString());
    const { offset, length } = header.traced_module_dependencies;
    fs.writeFileSync(
      filename,
      original.subarray(0, headerEnd + offset + length - 1),
    );
    await expect(collect()).rejects.toThrow(/analyzer.*(section|length)/i);
  });

  it("rejects report data symlinks that escape the saved report", async () => {
    const modulesPath = path.join(report, "data/modules.data");
    const outside = path.join(fixture, "outside.data");
    fs.renameSync(modulesPath, outside);
    fs.symlinkSync(outside, modulesPath);
    await expect(collect()).rejects.toThrow(/escapes report/i);
  });
});

function mutateBinary(
  filename: string,
  field: string,
  mutate: (section: Buffer) => void,
) {
  const file = path.join(report, filename);
  const bytes = fs.readFileSync(file);
  const start = 4 + bytes.readUInt32BE(0);
  const header = JSON.parse(bytes.subarray(4, start).toString());
  const { offset, length } = header[field];
  mutate(bytes.subarray(start + offset, start + offset + length));
  fs.writeFileSync(file, bytes);
}
