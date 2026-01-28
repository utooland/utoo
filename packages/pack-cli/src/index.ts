import { defineCommand, runMain } from "citty";
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
  },
});

export const run = () => runMain(main);
