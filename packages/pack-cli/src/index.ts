import { defineCommand, runMain } from "citty";
import analyze from "./commands/analyze";
import build from "./commands/build";
import dev from "./commands/dev";

const main = defineCommand({
  meta: {
    name: "up",
    version: "0.0.0",
    description: "Utoo pack cli",
  },
  subCommands: {
    build,
    dev,
    analyze,
  },
});

export const run = () => runMain(main);
