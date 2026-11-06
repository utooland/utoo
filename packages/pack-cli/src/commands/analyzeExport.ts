import { writeSync } from "node:fs";
import path from "node:path";
import { exportAnalyzeReport } from "@utoo/pack/analyze";
import { defineCommand } from "citty";

export default defineCommand({
  meta: {
    name: "up analyze export",
    description:
      "Export a saved Analyzer report as JSON Lines without compiling",
  },
  args: {
    report: {
      type: "string",
      default: ".utoo/analyze",
      description: "Saved report directory, relative to the current directory",
    },
    entry: {
      type: "string",
      description: "Entry name or /entry-N ID (default: all entries)",
    },
    environment: {
      type: "string",
      description: "Filter entries by client or server",
    },
  },
  async run({ args }) {
    if (
      args.environment !== undefined &&
      args.environment !== "client" &&
      args.environment !== "server"
    ) {
      throw new Error("Analyzer environment must be 'client' or 'server'.");
    }
    // Match Next's export writer: block pipe writes rather than buffering the
    // whole dump or losing records when the CLI exits with pending writes.
    const stdout = process.stdout as NodeJS.WriteStream & {
      _handle?: { setBlocking?: (blocking: boolean) => void };
    };
    stdout._handle?.setBlocking?.(true);
    try {
      for await (const record of exportAnalyzeReport(
        path.resolve(args.report),
        {
          entry: args.entry,
          environment: args.environment,
        },
      )) {
        const line = Buffer.from(`${JSON.stringify(record)}\n`);
        let offset = 0;
        while (offset < line.length) {
          const written = writeSync(1, line, offset, line.length - offset);
          if (written === 0)
            throw new Error("Could not write Analyzer export.");
          offset += written;
        }
      }
    } catch (error) {
      // Allow consumers such as `head` to stop reading a large export early.
      if ((error as NodeJS.ErrnoException).code !== "EPIPE") throw error;
    }
  },
});
