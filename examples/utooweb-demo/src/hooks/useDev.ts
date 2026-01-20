import { Project as UtooProject } from "@utoo/web";
import { useCallback, useEffect, useRef, useState } from "react";
import { FileTreeNode } from "../types";
import { generateHtml } from "../utils/htmlGenerator";

// HMR server interface for type safety
interface HmrServerLike {
  sendBuilding: () => void;
  sendBuilt: (errors?: readonly unknown[]) => void;
  sendReload: (reason: string) => void;
}

export interface UseDevOptions {
  /** Auto start dev mode when project is ready */
  autoStart?: boolean;
  /** Callback when build completes successfully */
  onBuildComplete?: () => void;
}

export const useDev = (
  project: UtooProject | null,
  fileTree: FileTreeNode[],
  handleDirectoryExpand: (root: FileTreeNode) => Promise<void>,
  options: UseDevOptions = {},
) => {
  const { autoStart = false, onBuildComplete } = options;

  const [isDevMode, setIsDevMode] = useState(false);
  const [isBuilding, setIsBuilding] = useState(false);
  const [error, setError] = useState("");
  const [buildCount, setBuildCount] = useState(0);

  // Track if we've done initial build
  const initialBuildDone = useRef(false);
  const isStarting = useRef(false);

  // Get HMR server if available
  const getHmrServer = useCallback((): HmrServerLike | undefined => {
    return (project as unknown as { hmrServer?: HmrServerLike })?.hmrServer;
  }, [project]);

  // Process build output (generate HTML, refresh file tree)
  const processBuildOutput = useCallback(async () => {
    if (!project) return;

    try {
      const statsContent = await project.readFile("dist/stats.json", "utf8");
      const stats = JSON.parse(statsContent);

      const styles: string[] = [];
      const scripts: string[] = [];

      if (stats.assets) {
        for (const asset of stats.assets) {
          const assetPath = `/preview/dist/${asset.name}`;
          if (asset.name.endsWith(".css")) {
            styles.push(`<link rel="stylesheet" href="${assetPath}">`);
          } else if (asset.name.endsWith(".js")) {
            scripts.push(`<script src="${assetPath}"></script>`);
          }
        }
      }

      const html = generateHtml(styles, scripts);
      await project.writeFile("dist/index.html", html);

      // Refresh file tree
      const root = fileTree.find((node) => node.fullName === ".");
      if (root) {
        await handleDirectoryExpand(root);
      }

      onBuildComplete?.();
    } catch (e: any) {
      console.error("Failed to process build output:", e);
    }
  }, [project, fileTree, handleDirectoryExpand, onBuildComplete]);

  // Single build function
  const runBuild = useCallback(async (): Promise<boolean> => {
    if (!project) return false;

    const hmrServer = getHmrServer();
    setIsBuilding(true);
    hmrServer?.sendBuilding();

    try {
      const start = performance.now();
      await project.dev();
      const duration = Math.round(performance.now() - start);

      console.log(
        `%cDev:%c Built in ${duration}ms`,
        "color: blue;",
        "color: green",
      );

      hmrServer?.sendBuilt();
      setBuildCount((c) => c + 1);

      await processBuildOutput();

      return true;
    } catch (e: any) {
      console.error("Build failed:", e);
      setError(`Build failed: ${e.message || JSON.stringify(e)}`);
      hmrServer?.sendBuilt([{ message: e.message || String(e) }]);
      return false;
    } finally {
      setIsBuilding(false);
    }
  }, [project, getHmrServer, processBuildOutput]);

  // Start dev mode
  const startDev = useCallback(async () => {
    if (!project || isDevMode || isStarting.current) return;

    isStarting.current = true;
    setError("");

    try {
      console.log(
        "%cDev:%c Starting dev mode...",
        "color: blue;",
        "color: gray",
      );

      // Do initial build
      const success = await runBuild();
      if (success) {
        initialBuildDone.current = true;
        setIsDevMode(true);
        console.log(
          "%cDev:%c Dev mode started. Edit files and click rebuild, or implement file watching.",
          "color: blue;",
          "color: green",
        );
      }
    } catch (e: any) {
      console.error("Failed to start dev mode:", e);
      setError(`Failed to start dev mode: ${e.message}`);
    } finally {
      isStarting.current = false;
    }
  }, [project, isDevMode, runBuild]);

  // Stop dev mode
  const stopDev = useCallback(() => {
    setIsDevMode(false);
    initialBuildDone.current = false;
    console.log("%cDev:%c Dev mode stopped", "color: blue;", "color: gray");
  }, []);

  // Rebuild (manual trigger for incremental builds)
  const rebuild = useCallback(async () => {
    if (!project || !isDevMode) return;
    await runBuild();
  }, [project, isDevMode, runBuild]);

  // Auto start if enabled
  useEffect(() => {
    if (autoStart && project && !isDevMode && !isStarting.current) {
      startDev();
    }
  }, [autoStart, project, isDevMode, startDev]);

  return {
    /** Whether dev mode is active */
    isDevMode,
    /** Whether a build is in progress */
    isBuilding,
    /** Current error message */
    error,
    /** Number of successful builds */
    buildCount,
    /** Start dev mode (initial build) */
    startDev,
    /** Stop dev mode */
    stopDev,
    /** Trigger a rebuild */
    rebuild,
  };
};
