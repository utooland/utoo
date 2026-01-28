import * as utooPack from "@utoo/pack";
import { defineCommand } from "citty";
import { resolveBuildOptions } from "../utils/common";

export default defineCommand({
  meta: {
    name: "dev",
    description: "utoopack dev",
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
  },
  async run({ args }) {
    const { projectOptions, projectPath, rootPath } = resolveBuildOptions(args);
    await utooPack.serve(projectOptions, projectPath, rootPath, {
      logServerInfo: true,
      port: 3000,
    });
  },
});
