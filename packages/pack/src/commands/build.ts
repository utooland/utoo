import { handleIssues } from "@utoo/pack-shared";
import { spawn } from "child_process";
import fs, { existsSync } from "fs";
import { nanoid } from "nanoid";
import { join } from "path";
import { ConfigComplete } from "../config/types";
import {
  compatOptionsFromWebpack,
  WebpackConfig,
} from "../config/webpackCompat";
import { projectFactory } from "../core/project";
import { BundleOptions } from "../core/types";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { blockStdout, createDefineEnv, getPackPath } from "../utils/common";
import { findRootDir } from "../utils/find-root";
import { processHtmlEntry } from "../utils/html-entry";
import { xcodeProfilingReady } from "../utils/xcodeProfile";

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

  processHtmlEntry(bundleOptions.config, projectPath || process.cwd());

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
        stats:
          Boolean(process.env.ANALYZE) ||
          bundleOptions.config.stats ||
          bundleOptions.config.entry.some((e) => !!e.html),
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

  const htmlConfigs = [
    ...(Array.isArray((bundleOptions.config as any).html)
      ? (bundleOptions.config as any).html
      : (bundleOptions.config as any).html
        ? [(bundleOptions.config as any).html]
        : []),
    ...bundleOptions.config.entry.filter((e) => !!e.html).map((e) => e.html!),
  ];

  if (htmlConfigs.length > 0) {
    const assets = { js: [] as string[], css: [] as string[] };
    // if (entrypoints.apps) {
    //   for (const app of entrypoints.apps) {
    //     const written = await app.writeToDisk();
    //     written.clientPaths.forEach((p) => {
    //       if (p.endsWith(".js")) assets.js.push(p);
    //       if (p.endsWith(".css")) assets.css.push(p);
    //     });
    //   }
    // }

    const outputDir =
      bundleOptions.config.output?.path || join(process.cwd(), "dist");

    if (assets.js.length === 0 && assets.css.length === 0) {
      const statsPath = join(outputDir, "stats.json");
      if (existsSync(statsPath)) {
        try {
          const stats = JSON.parse(fs.readFileSync(statsPath, "utf-8"));
          if (stats.assets) {
            stats.assets.forEach((asset: any) => {
              if (asset.name.endsWith(".js")) assets.js.push(asset.name);
              if (asset.name.endsWith(".css")) assets.css.push(asset.name);
            });
          }
        } catch (e) {
          console.warn("Failed to read stats.json for assets discovery", e);
        }
      }
    }

    const publicPath = bundleOptions.config.output?.publicPath;

    for (const config of htmlConfigs) {
      const plugin = new HtmlPlugin(config);
      await plugin.generate(outputDir, assets, publicPath);
    }
  }

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
