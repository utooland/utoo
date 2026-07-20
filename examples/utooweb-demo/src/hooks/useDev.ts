import { Project as UtooProject } from "@utoo/web";
import { useCallback, useEffect, useRef, useState } from "react";
import { FileTreeNode } from "../types";
import { generateHtml } from "../utils/htmlGenerator";

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

  // Track if we've started dev mode
  const isStarting = useRef(false);

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

  // Start dev mode - calls project.dev() with an onUpdate callback
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

      setIsBuilding(true);

      project.dev({
        onUpdate: (result) => {
          console.log(
            `%cDev:%c Build completed`,
            "color: blue;",
            "color: green",
          );

          setIsBuilding(false);
          setBuildCount((c) => c + 1);

          // Process build output
          processBuildOutput();

          // Mark dev mode as active after first build
          if (!isDevMode) {
            setIsDevMode(true);
            console.log(
              "%cDev:%c Dev mode started. Watching for file changes...",
              "color: blue;",
              "color: green",
            );
          }

          // Signal next build is starting
          setIsBuilding(true);
        },
      });
    } catch (e: any) {
      console.error("Failed to start dev mode:", e);
      setError(`Failed to start dev mode: ${e.message}`);
      isStarting.current = false;
    }
  }, [project, isDevMode, processBuildOutput]);

  // Stop dev mode (note: currently no way to stop the subscription)
  const stopDev = useCallback(() => {
    setIsDevMode(false);
    setIsBuilding(false);
    console.log("%cDev:%c Dev mode stopped", "color: blue;", "color: gray");
  }, []);

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
    /** Start dev mode (initial build + file watching) */
    startDev,
    /** Stop dev mode */
    stopDev,
  };
};
