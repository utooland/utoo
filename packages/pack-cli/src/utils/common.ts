import { Flags } from "@oclif/core";
import * as utooPack from "@utoo/pack";
import fs from "fs";
import path from "path";

export const commonFlags = {
  project: Flags.string({
    char: "p",
    description: "Set the project path",
    required: false,
  }),
  root: Flags.string({
    char: "r",
    description: "Set the root path",
    required: false,
  }),
  webpack: Flags.boolean({
    name: "webpack",
    description: "Enable webpack mode",
    required: false,
  }),
};

export interface BuildOptions {
  projectPath: string;
  rootPath: string | undefined;
  projectOptions: utooPack.WebpackConfig | utooPack.BundleOptions;
}

export function resolveBuildOptions(flags: {
  project?: string;
  root?: string;
  webpack?: boolean;
}): BuildOptions {
  const { project, root, webpack } = flags;
  const cwd = process.cwd();
  let projectPath = path.resolve(cwd, project || cwd);
  let rootPath = root ? path.resolve(cwd, root) : undefined;

  let projectOptions: utooPack.WebpackConfig | utooPack.BundleOptions;

  if (webpack) {
    projectOptions = { webpackMode: true } as utooPack.WebpackConfig;
  } else {
    projectOptions = JSON.parse(
      fs.readFileSync(
        path.resolve(cwd, project || "", "project_options.json"),
        {
          encoding: "utf-8",
        },
      ),
    ) as utooPack.BundleOptions;
  }

  return { projectPath, rootPath, projectOptions };
}
