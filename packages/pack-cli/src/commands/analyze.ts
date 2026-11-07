import * as utooPack from "@utoo/pack";
import { defineCommand } from "citty";
import { resolveBuildOptions } from "../utils/common";

export default defineCommand({
  meta: {
    name: "analyze",
    description:
      "Analyze client/server bundles; use 'up analyze export' to read a saved report as JSONL",
  },
  args: {
    project: {
      type: "string",
      alias: "p",
      description: "Set the project path",
    },
    root: { type: "string", alias: "r", description: "Set the root path" },
    webpack: {
      type: "boolean",
      description: "Read Webpack-compatible configuration",
    },
    tracing: {
      type: "boolean",
      description: "Show analysis progress (pass --no-tracing to disable)",
    },
    output: {
      type: "boolean",
      alias: "o",
      description: "Write the report without starting a server",
    },
    "output-dir": {
      type: "string",
      description:
        "Report directory relative to the project (default: .utoo/analyze)",
    },
    port: {
      type: "string",
      default: "4000",
      description: "Local report server port",
    },
  },
  async run({ args }) {
    const port = Number(args.port);
    if (!/^\d+$/.test(args.port) || !Number.isInteger(port) || port > 65535) {
      throw new Error("Analyzer port must be an integer between 0 and 65535.");
    }
    const { projectOptions, projectPath, rootPath } =
      await resolveBuildOptions(args);
    const report = await utooPack.analyze(
      projectOptions,
      projectPath,
      rootPath,
      {
        outputPath: args["output-dir"],
      },
    );
    console.log(`Analyzer report written to ${report.outputPath}`);
    if (!args.output) {
      const server = await utooPack.serveAnalyzeReport(report.outputPath, port);
      console.log(`Bundle Analyzer available at ${server.url}`);
    }
  },
});
