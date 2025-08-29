import { useState, useCallback, useEffect, useRef } from "react";
import { Project as UtooProject } from "@utoo/web";
import { serviceWorkerScope } from "../services/utooService";

export const useFileContent = (project: UtooProject | null) => {
    const [selectedFilePath, setSelectedFilePath] = useState("");
    const [selectedFileContent, setSelectedFileContent] = useState("");
    const [previewUrl, setPreviewUrl] = useState<string>("");
    const [error, setError] = useState("");
    const debounceTimer = useRef<NodeJS.Timeout | null>(null);

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

    useEffect(() => {
        if (debounceTimer.current) {
            clearTimeout(debounceTimer.current);
        }

        if (project && selectedFilePath && selectedFileContent) {
            debounceTimer.current = setTimeout(async () => {
                try {
                    await project.writeFile(selectedFilePath, selectedFileContent);
                    console.log(`File ${selectedFilePath} auto-saved successfully.`);
                } catch (e: any) {
                    setError(`Error auto-saving file: ${e.message}`);
                }
            }, 300);
        }

        return () => {
            if (debounceTimer.current) {
                clearTimeout(debounceTimer.current);
            }
        };
    }, [selectedFileContent, selectedFilePath, project]);


    return { selectedFilePath, selectedFileContent, setSelectedFileContent, previewUrl, fetchFileContent, error };
};
