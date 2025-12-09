import { handleIssues } from "@utoo/pack-shared";
import { spawn } from "child_process";
import { existsSync } from "fs";
import { nanoid } from "nanoid";
import { join } from "path";
import { findRootDir } from "./find-root";
import { projectFactory } from "./project";
import { BundleOptions } from "./types";
import { blockStdout, createDefineEnv, getPackPath } from "./util";
import { compatOptionsFromWebpack, WebpackConfig } from "./webpackCompat";
import { xcodeProfilingReady } from "./xcodeProfile";

export function build(
  bundleOptions: BundleOptions,
  projectPath?: string,
  rootPath?: string,
): Promise<void>;

export function build(
  webpackConfig: WebpackConfig,
  projectPath?: string,
  rootPath?: string,
): Promise<void>;

export function build(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
) {
  const bundleOptions = (<WebpackConfig>options).webpackMode
    ? compatOptionsFromWebpack(<WebpackConfig>options, projectPath, rootPath)
    : <BundleOptions>options;
  if (!rootPath) {
    // help user to find the rootDir automatically.
    rootPath = findRootDir(projectPath || process.cwd());
  }
  return buildInternal(bundleOptions, projectPath, rootPath);
}

async function buildInternal(
  bundleOptions: BundleOptions,
  projectPath?: string,
  rootPath?: string,
) {
  blockStdout();

  if (process.env.XCODE_PROFILE) {
    await xcodeProfilingReady();
  }

  const createProject = projectFactory();
  const project = await createProject(
    {
      processEnv: bundleOptions.processEnv ?? {},
      defineEnv: createDefineEnv({
        config: bundleOptions.config,
        dev: bundleOptions.dev ?? false,
        optionDefineEnv: bundleOptions.defineEnv,
      }),
      watch: {
        enable: false,
      },
      dev: bundleOptions.dev ?? false,
      buildId: bundleOptions.buildId || nanoid(),
      config: {
        ...bundleOptions.config,
        stats: Boolean(process.env.ANALYZE) || bundleOptions.config.stats,
      },
      projectPath: projectPath || process.cwd(),
      rootPath: rootPath || projectPath || process.cwd(),
      packPath: getPackPath(),
    },
    {
      persistentCaching: false,
    },
  );

  const entrypoints = await project.writeAllEntrypointsToDisk();

  handleIssues(entrypoints.issues);

  if (process.env.ANALYZE) {
    await analyzeBundle(bundleOptions.config.output?.path || "dist");
  }
  await project.shutdown();

  // TODO: Maybe run tasks in worker is a better way, see
  // https://github.com/vercel/next.js/blob/512d8283054407ab92b2583ecce3b253c3be7b85/packages/next/src/lib/worker.ts
}

async function analyzeBundle(outputPath: string): Promise<void> {
  const statsPath = join(outputPath, "stats.json");

  if (!existsSync(statsPath)) {
    console.warn(
      `Stats file not found at ${statsPath}. Make sure to enable stats in your configuration.`,
    );
    return;
  }

  return new Promise((resolve, reject) => {
    const analyzer = spawn("npx", ["webpack-bundle-analyzer", statsPath], {
      stdio: "inherit",
      shell: true,
    });

    analyzer.on("error", (error) => {
      reject(new Error(`Failed to start bundle analyzer: ${error.message}`));
    });

    analyzer.on("close", () => {
      // The analyzer process has finished, so we can resolve the promise
      // to allow the build process to exit gracefully.
      resolve();
    });
  });
}
