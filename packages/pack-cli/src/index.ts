import { defineCommand, runMain } from "citty";
import exportAnalyze from "./commands/analyzeExport";

const main = defineCommand({
  meta: {
    name: "up",
    version: "0.0.0",
    description: "Utoo pack cli",
  },
  subCommands: {
    build: () => import("./commands/build").then((module) => module.default),
    dev: () => import("./commands/dev").then((module) => module.default),
    analyze: () =>
      import("./commands/analyze").then((module) => module.default),
  },
});

export const run = (rawArgs = process.argv.slice(2)) => {
  // Citty runs parent handlers after subcommands and treats option values as
  // subcommand names. Dispatch replay separately so it never starts a capture.
  if (rawArgs[0] === "analyze" && rawArgs[1] === "export") {
    return runMain(exportAnalyze, { rawArgs: rawArgs.slice(2) });
  }
  return runMain(main, { rawArgs });
};
