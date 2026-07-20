import { EntryOptions, handleIssues } from "@utoo/pack-shared";
import { spawn } from "child_process";
import fs from "fs";
import { nanoid } from "nanoid";
import path from "path";
import { BundleOptions } from "../config/types";
import { resolveBundleOptions, WebpackConfig } from "../config/webpackCompat";
import { projectFactory } from "../core/project";
import { HtmlPlugin } from "../plugins/HtmlPlugin";
import { cleanOutput, getOutputPath } from "../utils/cleanOutput";
import { blockStdout, getPackPath } from "../utils/common";
import {
  isPersistentCachingEnabled,
  isTruthyEnv,
  normalizeBuildTurbopackMemoryEviction,
} from "../utils/env";
import { findRootDir } from "../utils/findRoot";
import { getInitialAssetsFromEndpointPaths } from "../utils/getInitialAssets";
import { processHtmlEntry } from "../utils/htmlEntry";
import { acquirePersistentCacheLock } from "../utils/lockfile";
import { normalizePath } from "../utils/normalizePath";
import { useWorkerThreads } from "../utils/runtimePluginStratety";
import { validateEntryPaths } from "../utils/validateEntry";
import { xcodeProfilingReady } from "../utils/xcodeProfile";

type MemoryEvictionMode = import("../binding").MemoryEvictionMode;

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
  blockStdout();

  if (process.env.XCODE_PROFILE) {
    await xcodeProfilingReady();
  }

  const resolvedProjectPath = projectPath || process.cwd();
  const resolvedRootPath = rootPath || projectPath || process.cwd();
  const persistentCaching = isPersistentCachingEnabled(
    bundleOptions.config.persistentCaching,
  );
  const turbopackMemoryEviction = normalizeBuildTurbopackMemoryEviction(
    bundleOptions.config.turbopackMemoryEviction,
    process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT,
  ) as MemoryEvictionMode;
  const smallPreallocation = isTruthyEnv(
    process.env.UTOO_TURBOPACK_SMALL_PREALLOCATION,
  );
  const shouldCreateWebpackStats =
    Boolean(process.env.ANALYZE) || Boolean(bundleOptions.config.stats);
  processHtmlEntry(bundleOptions.config, resolvedProjectPath);
  validateEntryPaths(bundleOptions.config, resolvedProjectPath);
  await cleanOutput(bundleOptions.config, resolvedProjectPath);

  const createProject = projectFactory();
  const persistentCacheLock = await acquirePersistentCacheLock(
    resolvedProjectPath,
    "utoo pack build",
    persistentCaching,
  );

  let project:
    | Awaited<ReturnType<ReturnType<typeof projectFactory>>>
    | undefined;
  try {
    project = await createProject(
      {
        processEnv: bundleOptions.processEnv ?? {},
        watch: {
          enable: false,
        },
        dev: bundleOptions.dev ?? false,
        buildId: bundleOptions.buildId || nanoid(),
        tracing: bundleOptions.tracing ?? true,
        config: {
          ...bundleOptions.config,
          stats: shouldCreateWebpackStats,
          pluginRuntimeStrategy:
            bundleOptions?.config?.pluginRuntimeStrategy ??
            (useWorkerThreads() ? "workerThreads" : "childProcesses"),
        },
        projectPath: normalizePath(resolvedProjectPath),
        rootPath: resolvedRootPath,
        packPath: getPackPath(),
      },
      {
        persistentCaching,
        turbopackMemoryEviction,
        smallPreallocation,
        // Build mode is a short-lived, one-shot compilation, so avoid paying
        // dependency graph bookkeeping cost unless the persistent cache needs it.
        dependencyTracking: persistentCaching,
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
      ...bundleOptions.config.entry
        .filter((e: EntryOptions) => !!e.html)
        .map((e: EntryOptions) => e.html!),
    ];

    if (htmlConfigs.length > 0) {
      const assets = getInitialAssetsFromEndpointPaths([
        ...(entrypoints.appPaths ?? []),
        ...(entrypoints.libraryPaths ?? []),
      ]);

      const outputDir = getOutputPath(
        bundleOptions.config,
        resolvedProjectPath,
      );

      const publicPath = bundleOptions.config.output?.publicPath;

      for (const config of htmlConfigs) {
        const plugin = new HtmlPlugin(config);
        await plugin.generate(outputDir, assets, publicPath);
      }
    }

    if (process.env.ANALYZE) {
      await analyzeBundle(bundleOptions.config.output?.path || "dist");
    }
  } finally {
    await project?.shutdown();
    persistentCacheLock?.unlockSync();
  }

  // TODO: Maybe run tasks in worker is a better way, see
  // https://github.com/vercel/next.js/blob/512d8283054407ab92b2583ecce3b253c3be7b85/packages/next/src/lib/worker.ts
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
