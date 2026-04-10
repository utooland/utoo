import * as utooPack from "@utoo/pack";
import { defineCommand } from "citty";
import { resolveBuildOptions } from "../utils/common";

export default defineCommand({
  meta: {
    name: "build",
    description: "utoopack build",
  },
  args: {
    project: {
      type: "string",
      alias: "p",
      description: "Set the project path",
    },
    root: {
      type: "string",
      alias: "r",
      description: "Set the root path",
    },
    webpack: {
      type: "boolean",
      description: "Enable webpack mode",
    },
    analyze: {
      type: "boolean",
      description: "Generate native Turbopack analyze data",
    },
  },
  async run({ args }) {
    if (args.analyze && !process.env.ANALYZE) {
      process.env.ANALYZE = "native";
    }
    const { projectOptions, projectPath, rootPath } =
      await resolveBuildOptions(args);
    await utooPack.build(projectOptions, projectPath, rootPath);
  },
});
