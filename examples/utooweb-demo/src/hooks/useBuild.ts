import { useState, useCallback } from "react";
import { Project as UtooProject } from "@utoo/web";
import { FileTreeNode } from "../types";
import { generateHtml } from "../utils/htmlGenerator";

export const useBuild = (project: UtooProject | null, fileTree: FileTreeNode[], handleDirectoryExpand: (root: FileTreeNode) => Promise<void>) => {
    const [isBuilding, setIsBuilding] = useState(false);
    const [error, setError] = useState("");

    const handleBuild = useCallback(async () => {
        if (!project) return;
        setIsBuilding(true);
        setError("");
        try {
            await project.build();

            try {
                const statsContent = await project.readFile("dist/stats.json", "utf8");
                const stats = JSON.parse(statsContent);

                const styles: string[] = [];
                const scripts: string[] = [];

                if (stats.assets) {
                    for (const asset of stats.assets) {
                        const assetPath = `/dist/${asset.name}`;
                        if (asset.name.endsWith(".css")) {
                            styles.push(`<link rel="stylesheet" href="${assetPath}">`);
                        } else if (asset.name.endsWith(".js")) {
                            scripts.push(`<script src="${assetPath}"></script>`);
                        }
                    }
                }

                const html = generateHtml(styles, scripts);

                await project.writeFile("dist/index.html", html);

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
            setError(`Build failed: ${e.message}`);
        } finally {
            setIsBuilding(false);
        }
    }, [project, fileTree, handleDirectoryExpand]);

    return { isBuilding, handleBuild, error };
};
