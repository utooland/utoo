import { useState, useCallback } from "react";
import { Project as UtooProject } from "@utoo/web";
import { serviceWorkerScope } from "../services/utooService";

export const useFileContent = (project: UtooProject | null) => {
    const [selectedFilePath, setSelectedFilePath] = useState("");
    const [selectedFileContent, setSelectedFileContent] = useState("");
    const [previewUrl, setPreviewUrl] = useState<string>("");
    const [error, setError] = useState("");

    const fetchFileContent = useCallback(
        async (filePath: string): Promise<void> => {
            setSelectedFilePath(filePath);
            setSelectedFileContent("");
            try {
                if (!project) throw new Error("Project not initialized.");

                const content: string = await project.readFile(filePath, "utf8");
                setSelectedFileContent(content);

                if (filePath.endsWith("dist/index.html")) {
                    setPreviewUrl(`${location.origin}${serviceWorkerScope}/${filePath}`);
                }
            } catch (e: any) {
                setError(`Error reading file: ${e.message}`);
            }
        },
        [project],
    );

    return { selectedFilePath, selectedFileContent, previewUrl, fetchFileContent, error };
};
