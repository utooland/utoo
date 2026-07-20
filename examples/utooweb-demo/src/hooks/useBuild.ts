import { Project as UtooProject } from "@utoo/web";
import { useCallback, useState } from "react";
import { FileTreeNode } from "../types";
import { generateHtml } from "../utils/htmlGenerator";

// HMR server interface for type safety (will be available after utoo-web is rebuilt)
interface HmrServerLike {
  sendBuilding: () => void;
  sendBuilt: (errors?: readonly unknown[]) => void;
}

export const useBuild = (
  project: UtooProject | null,
  fileTree: FileTreeNode[],
  handleDirectoryExpand: (root: FileTreeNode) => Promise<void>,
  onBuildComplete: () => void,
) => {
  const [isBuilding, setIsBuilding] = useState(false);
  const [error, setError] = useState("");

  const handleBuild = useCallback(async () => {
    if (!project) return;
    setIsBuilding(true);
    setError("");

    // Get HMR server if available (type assertion for forward compatibility)
    const hmrServer = (project as unknown as { hmrServer?: HmrServerLike })
      .hmrServer;

    // Notify HMR clients that build is starting
    hmrServer?.sendBuilding();

    try {
      const start = performance.now();
      await project.build({ cleanup: true });
      console.log(
        `%cPack Project:%c Finished to build in ${Math.round(performance.now() - start)} ms.`,
        "color: blue;",
        "color: green",
      );

      // Notify HMR clients that build completed
      hmrServer?.sendBuilt();

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
        onBuildComplete();

        const root = fileTree.find((node) => node.fullName === ".");
        if (root) {
          await handleDirectoryExpand(root);
        }
      } catch (e: any) {
        console.error("Failed to process stats.json:", e);
        setError(`Build succeeded, but failed to display stats: ${e.message}`);
      }
    } catch (e: any) {
      console.error("Build failed: ", e);
      setError(`Build failed: ${JSON.stringify(e)}`);

      // Notify HMR clients about build error
      hmrServer?.sendBuilt([{ message: e.message || String(e) }]);
    } finally {
      setIsBuilding(false);
    }
  }, [project, fileTree, handleDirectoryExpand, onBuildComplete]);

  return { isBuilding, handleBuild, error };
};
