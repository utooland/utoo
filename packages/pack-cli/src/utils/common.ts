import * as utooPack from "@utoo/pack";
import fs from "fs";
import path from "path";

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
    const rawOptions = JSON.parse(
      fs.readFileSync(path.resolve(cwd, project || "", "utoopack.json"), {
        encoding: "utf-8",
      }),
    );
    const {
      processEnv,
      defineEnv,
      watch,
      dev,
      buildId,
      packPath,
      rootPath: _rootPath,
      projectPath: _projectPath,
      ...config
    } = rawOptions;
    projectOptions = {
      config,
      processEnv,
      defineEnv,
      watch,
      dev,
      buildId,
      packPath,
    } as utooPack.BundleOptions;
  }

  return { projectPath, rootPath, projectOptions };
}
