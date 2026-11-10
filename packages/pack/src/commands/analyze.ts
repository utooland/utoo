import {
  cp,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import path from "node:path";
import { handleIssues } from "@utoo/pack-shared";
import { nanoid } from "nanoid";
import type { BundleOptions } from "../config/types";
import {
  resolveBundleOptions,
  type WebpackConfig,
} from "../config/webpackCompat";
import { projectFactory } from "../core/project";
import type { AnalyzeData, Project } from "../core/types";
import { resolveCacheDirectory } from "../utils/cacheDirectory";
import { getOutputPath } from "../utils/cleanOutput";
import { blockStdout, getPackPath } from "../utils/common";
import {
  isPersistentCachingEnabled,
  isTruthyEnv,
  normalizeTurbopackMemoryEviction,
} from "../utils/env";
import { resolveRootPath } from "../utils/findRoot";
import { processHtmlEntry } from "../utils/htmlEntry";
import { acquirePersistentCacheLock } from "../utils/lockfile";
import { normalizePath } from "../utils/normalizePath";
import { useWorkerThreads } from "../utils/runtimePluginStratety";
import {
  formatDuration,
  formatTaskCount,
  TaskProgress,
} from "../utils/taskProgress";
import { validateEntryPaths } from "../utils/validateEntry";

export interface AnalyzeOptions {
  /** Report directory, resolved from projectPath. Defaults to .utoo/analyze. */
  outputPath?: string;
}

export interface AnalyzeResult extends AnalyzeData {
  outputPath: string;
}

const REPORT_MARKER = "utoo-analyzer.json";

function overlaps(a: string, b: string): boolean {
  const contains = (parent: string, child: string) => {
    const relative = path.relative(parent, child);
    return (
      relative === "" ||
      (!relative.startsWith(`..${path.sep}`) &&
        relative !== ".." &&
        !path.isAbsolute(relative))
    );
  };
  return contains(a, b) || contains(b, a);
}

async function validateReportDirectory(outputPath: string): Promise<void> {
  try {
    const info = await lstat(outputPath);
    if (!info.isDirectory() || info.isSymbolicLink()) {
      throw new Error(
        `Analyzer output must be a report directory: ${outputPath}`,
      );
    }
    const marker = JSON.parse(
      await readFile(path.join(outputPath, REPORT_MARKER), "utf8"),
    );
    if (marker.tool !== "@utoo/pack-analyzer") {
      throw new Error("Not an Analyzer report");
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      // A missing marker in an existing directory is not permission to replace it.
      try {
        await lstat(outputPath);
      } catch (statError) {
        if ((statError as NodeJS.ErrnoException).code === "ENOENT") return;
        throw statError;
      }
    }
    throw new Error(
      `Analyzer output already exists and is not a Pack Analyzer report: ${outputPath}`,
      { cause: error },
    );
  }
}

/**
 * Analyze a production application build using the native module graph and output
 * source maps. The caller's configuration, bundles and Webpack stats are left intact.
 */
export async function analyze(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
  analyzeOptions: AnalyzeOptions = {},
): Promise<AnalyzeResult> {
  blockStdout();
  const resolvedProjectPath = path.resolve(projectPath || process.cwd());
  const bundleOptions = resolveBundleOptions(
    options,
    resolvedProjectPath,
    rootPath,
  );
  const config = {
    ...bundleOptions.config,
    entry: bundleOptions.config.entry?.map((entry) => ({ ...entry })) ?? [],
    mode: "production" as const,
    sourceMaps: true,
    stats: false,
  };
  processHtmlEntry(config, resolvedProjectPath);
  validateEntryPaths(config, resolvedProjectPath);
  const serverEntries =
    typeof config.server?.entry === "string"
      ? [{ import: config.server.entry }]
      : (config.server?.entry ?? []);
  if (!config.entry.length && !serverEntries.length) {
    throw new Error("Native Analyzer requires at least one application entry.");
  }
  if (config.entry.some((entry) => entry.library)) {
    throw new Error(
      "Native Analyzer supports application entries; library builds are not supported.",
    );
  }

  const outputPath = path.resolve(
    resolvedProjectPath,
    analyzeOptions.outputPath || ".utoo/analyze",
  );
  const buildOutputPaths = [getOutputPath(config, resolvedProjectPath)];
  if (config.server?.output?.path) {
    buildOutputPaths.push(
      path.resolve(resolvedProjectPath, config.server.output.path),
    );
  }
  if (buildOutputPaths.some((buildPath) => overlaps(outputPath, buildPath))) {
    throw new Error(
      "Analyzer output must be outside the application's build output directory.",
    );
  }
  await validateReportDirectory(outputPath);
  const uiPath = path.resolve(getPackPath(), "../analyzer");
  await readFile(path.join(uiPath, "index.html"));

  const cacheDirectory = resolveCacheDirectory(
    resolvedProjectPath,
    config.cacheDirectory,
  );
  const persistentCaching = isPersistentCachingEnabled(
    config.persistentCaching,
  );
  const lock = await acquirePersistentCacheLock(
    cacheDirectory,
    "utoo pack analyze",
    persistentCaching,
  );
  let project: Project | undefined;
  let stagingPath: string | undefined;
  try {
    await mkdir(path.dirname(outputPath), { recursive: true });
    stagingPath = await mkdtemp(
      path.join(path.dirname(outputPath), `.${path.basename(outputPath)}-`),
    );
    project = await projectFactory()(
      {
        config: {
          ...config,
          pluginRuntimeStrategy:
            config.pluginRuntimeStrategy ??
            (useWorkerThreads() ? "workerThreads" : "childProcesses"),
        },
        projectPath: normalizePath(resolvedProjectPath),
        rootPath: resolveRootPath(resolvedProjectPath, rootPath),
        processEnv: bundleOptions.processEnv ?? {},
        watch: { enable: false },
        dev: false,
        buildId: bundleOptions.buildId || nanoid(),
        tracing: bundleOptions.tracing ?? true,
        packPath: getPackPath(),
      },
      {
        persistentCaching,
        isShortSession: true,
        dependencyTracking: persistentCaching,
        cacheDirectory,
        turbopackMemoryEviction: normalizeTurbopackMemoryEviction(
          config.turbopackMemoryEviction,
        ) as import("../binding").MemoryEvictionMode,
        smallPreallocation: isTruthyEnv(
          process.env.UTOO_TURBOPACK_SMALL_PREALLOCATION,
        ),
        turbopackGc: config.turbopackGc,
      },
    );

    const progress =
      (bundleOptions.tracing ?? true) ? new TaskProgress(project) : undefined;
    const start = Date.now();
    progress?.start("Analyzing");
    let completedTasks = 0;
    const result = await project
      .writeAnalyzeData(path.join(stagingPath, "data"))
      .finally(() => {
        completedTasks = progress?.stop() ?? 0;
      });
    handleIssues(result.issues);
    // Stop the compilation session before copying or serving the static report.
    await project.shutdown();
    project = undefined;
    await cp(uiPath, stagingPath, { recursive: true });
    await writeFile(
      path.join(stagingPath, "data/entries.json"),
      JSON.stringify(result.entries),
    );
    await writeFile(
      path.join(stagingPath, REPORT_MARKER),
      JSON.stringify({ tool: "@utoo/pack-analyzer", version: 1 }),
    );
    await validateReportDirectory(outputPath);
    await rm(outputPath, { recursive: true, force: true });
    await rename(stagingPath, outputPath);
    stagingPath = undefined;
    if (progress) {
      console.log(
        `Analyzed in ${formatDuration(Date.now() - start)} (${formatTaskCount(completedTasks)})`,
      );
    }
    return { outputPath, entries: result.entries };
  } finally {
    try {
      await project?.shutdown();
    } finally {
      lock?.unlockSync();
      if (stagingPath) await rm(stagingPath, { recursive: true, force: true });
    }
  }
}
