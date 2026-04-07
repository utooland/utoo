import { EntryOptions, handleIssues } from "@utoo/pack-shared";
import { spawn } from "child_process";
import fs from "fs";
import { nanoid } from "nanoid";
import path from "path";
import { BundleOptions } from "../config/types";
import { resolveBundleOptions, WebpackConfig } from "../config/webpackCompat";
import { projectFactory } from "../core/project";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { blockStdout, getPackPath } from "../utils/common";
import { findRootDir } from "../utils/findRoot";
import { getInitialAssetsFromStats } from "../utils/getInitialAssets";
import { processHtmlEntry } from "../utils/htmlEntry";
import { normalizePath } from "../utils/normalize-path";
import { useWorkerThreads } from "../utils/runtimePluginStratety";
import { validateEntryPaths } from "../utils/validateEntry";
import { xcodeProfilingReady } from "../utils/xcodeProfile";

type AnalyzeMode = "none" | "native" | "webpack";

export function build(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
) {
  const bundleOptions = resolveBundleOptions(options, projectPath, rootPath);

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
  const analyzeMode = getAnalyzeMode();
  blockStdout();

  if (process.env.XCODE_PROFILE) {
    await xcodeProfilingReady();
  }

  const resolvedProjectPath = projectPath || process.cwd();
  const outputPath = resolveOutputPath(
    resolvedProjectPath,
    bundleOptions.config.output?.path,
  );
  processHtmlEntry(bundleOptions.config, resolvedProjectPath);
  validateEntryPaths(bundleOptions.config, resolvedProjectPath);

  const createProject = projectFactory();
  const project = await createProject(
    {
      processEnv: bundleOptions.processEnv ?? {},
      watch: {
        enable: false,
      },
      dev: bundleOptions.dev ?? false,
      buildId: bundleOptions.buildId || nanoid(),
      config: {
        ...bundleOptions.config,
        stats:
          analyzeMode === "webpack" ||
          bundleOptions.config.stats ||
          bundleOptions.config.entry.some((e: EntryOptions) => !!e.html),
        pluginRuntimeStrategy:
          bundleOptions?.config?.pluginRuntimeStrategy ??
          (useWorkerThreads() ? "workerThreads" : "childProcesses"),
      },
      projectPath: normalizePath(resolvedProjectPath),
      rootPath: rootPath || projectPath || process.cwd(),
      packPath: getPackPath(),
    },
    {
      persistentCaching: bundleOptions.config.persistentCaching ?? false,
    },
  );
  let nativeAnalyzeReady = false;

  try {
    const entrypoints = await project.writeAllEntrypointsToDisk();

    handleIssues(entrypoints.issues);

    const htmlConfigs = [
      ...(Array.isArray((bundleOptions.config as any).html)
        ? (bundleOptions.config as any).html
        : (bundleOptions.config as any).html
          ? [(bundleOptions.config as any).html]
          : []),
      ...bundleOptions.config.entry
        .filter((e: EntryOptions) => !!e.html)
        .map((e: EntryOptions) => e.html!),
    ];

    if (htmlConfigs.length > 0) {
      const assets = { js: [] as string[], css: [] as string[] };

      if (assets.js.length === 0 && assets.css.length === 0) {
        const discovered = getInitialAssetsFromStats(outputPath);
        assets.js.push(...discovered.js);
        assets.css.push(...discovered.css);
      }

      const publicPath = bundleOptions.config.output?.publicPath;

      for (const config of htmlConfigs) {
        const plugin = new HtmlPlugin(config);
        await plugin.generate(outputPath, assets, publicPath);
      }
    }

    if (analyzeMode === "native") {
      const analyzeResult = await project.writeAnalyzeData();
      handleIssues(analyzeResult.issues);
      nativeAnalyzeReady = true;
    } else if (analyzeMode === "webpack") {
      await analyzeBundle(outputPath);
    }
  } finally {
    await project.shutdown();
  }

  if (nativeAnalyzeReady) {
    const analyzeDataDir = path.join(
      outputPath,
      "diagnostics",
      "analyze",
      "data",
    );
    console.error(`Native analyze data written to ${analyzeDataDir}`);
  }

  // TODO: Maybe run tasks in worker is a better way, see
  // https://github.com/vercel/next.js/blob/512d8283054407ab92b2583ecce3b253c3be7b85/packages/next/src/lib/worker.ts
}

function getAnalyzeMode(): AnalyzeMode {
  const analyze = process.env.ANALYZE?.trim().toLowerCase();
  if (!analyze) {
    return "none";
  }
  if (analyze === "webpack" || analyze === "bundle" || analyze === "legacy") {
    return "webpack";
  }
  return "native";
}

function resolveOutputPath(
  projectPath: string,
  configuredOutputPath?: string,
): string {
  if (!configuredOutputPath) {
    return path.join(projectPath, "dist");
  }

  return path.isAbsolute(configuredOutputPath)
    ? configuredOutputPath
    : path.join(projectPath, configuredOutputPath);
}

async function analyzeBundle(outputPath: string): Promise<void> {
  const statsPath = path.join(outputPath, "stats.json");

  if (!fs.existsSync(statsPath)) {
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
