import { readFile } from "node:fs/promises";
import * as utooPack from "@utoo/pack";
import fs from "fs";
import path from "path";

const CONFIG_FILE_NAMES = [
  "utoopack.json",
  "utoopack.config.mjs",
  "utoopack.config.js",
] as const;

export interface BuildOptions {
  projectPath: string;
  rootPath: string | undefined;
  projectOptions: utooPack.WebpackConfig | utooPack.BundleOptions;
}

function resolveOptionalPath(
  baseDir: string,
  value: unknown,
): string | undefined {
  return typeof value === "string" ? path.resolve(baseDir, value) : undefined;
}

export async function resolveFromConfigFile(
  configDir: string,
): Promise<Record<string, unknown>> {
  const resolvedDir = path.resolve(configDir);
  for (const name of CONFIG_FILE_NAMES) {
    const configPath = path.join(resolvedDir, name);
    if (!fs.existsSync(configPath)) continue;

    if (name.endsWith(".json")) {
      const content = await readFile(configPath, "utf8");
      return JSON.parse(content) as Record<string, unknown>;
    }

    if (name.endsWith(".js") || name.endsWith(".mjs")) {
      const mod = (await import(configPath)) as
        | Record<string, unknown>
        | { default?: Record<string, unknown> };
      const raw =
        typeof mod?.default === "object" && mod.default !== null
          ? mod.default
          : mod;
      return raw as Record<string, unknown>;
    }
  }

  const tried = CONFIG_FILE_NAMES.map((n) => path.join(resolvedDir, n)).join(
    ", ",
  );
  console.error(`Error: Config file not found in ${resolvedDir}`);
  console.error(`Tried: ${tried}`);
  console.error(
    "Ensure utoopack.json or utoopack.config.mjs / utoopack.config.js exists in the project directory.",
  );
  process.exit(1);
}

export async function resolveBuildOptions(flags: {
  project?: string;
  root?: string;
  webpack?: boolean;
}): Promise<BuildOptions> {
  const { project, root, webpack } = flags;
  const cwd = process.cwd();
  const configDir = path.resolve(cwd, project || "");
  let projectPath = project ? path.resolve(cwd, project) : undefined;
  let rootPath = root ? path.resolve(cwd, root) : undefined;

  let projectOptions: utooPack.WebpackConfig | utooPack.BundleOptions;

  if (webpack) {
    projectOptions = { webpackMode: true } as utooPack.WebpackConfig;
  } else {
    const rawOptions = await resolveFromConfigFile(configDir);
    const {
      processEnv,
      watch,
      dev,
      buildId,
      packPath,
      rootPath: configRootPath,
      projectPath: configProjectPath,
      ...config
    } = rawOptions;

    projectPath =
      projectPath ??
      resolveOptionalPath(configDir, configProjectPath) ??
      configDir;
    rootPath = rootPath || resolveOptionalPath(configDir, configRootPath);

    projectOptions = {
      config,
      processEnv,
      watch,
      dev,
      buildId,
      packPath,
    } as unknown as utooPack.BundleOptions;
  }

  return { projectPath: projectPath ?? configDir, rootPath, projectOptions };
}
