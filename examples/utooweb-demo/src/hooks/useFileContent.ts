import { Project as UtooProject } from "@utoo/web";
import { useCallback, useEffect, useRef, useState } from "react";
import { serviceWorkerScope } from "../services/utooService";

export const useFileContent = (project: UtooProject | null) => {
  const [selectedFilePath, setSelectedFilePath] = useState("src/index.tsx");
  const [selectedFileContent, setSelectedFileContent] = useState("");
  const [originalContent, setOriginalContent] = useState("");
  const [previewUrl, setPreviewUrl] = useState<string>("");
  const [error, setError] = useState("");
  const [isSaving, setIsSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const justSavedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const hasUnsavedChanges = selectedFileContent !== originalContent;

  const fetchFileContent = useCallback(
    async (filePath: string): Promise<void> => {
      setSelectedFilePath(filePath);
      setSelectedFileContent("");
      setOriginalContent("");
      setJustSaved(false);
      try {
        if (!project) throw new Error("Project not initialized.");

        const content: string = await project.readFile(filePath, "utf8");
        setSelectedFileContent(content);
        setOriginalContent(content);

        if (filePath.endsWith("dist/index.html")) {
          setPreviewUrl(`${location.origin}${serviceWorkerScope}/${filePath}`);
        }
      } catch (e: any) {
        setError(`Error reading file: ${e.message}`);
      }
    },
    [project],
  );

  const saveFile = useCallback(async () => {
    if (!project || !selectedFilePath || !hasUnsavedChanges) return;

    setIsSaving(true);
    setError("");
    try {
      await project.writeFile(selectedFilePath, selectedFileContent);
      setOriginalContent(selectedFileContent);
      console.log(`File ${selectedFilePath} saved successfully.`);

      // Show "Saved" status briefly
      setJustSaved(true);
      if (justSavedTimerRef.current) {
        clearTimeout(justSavedTimerRef.current);
      }
      justSavedTimerRef.current = setTimeout(() => {
        setJustSaved(false);
      }, 2000);
    } catch (e: any) {
      setError(`Error saving file: ${e.message}`);
    } finally {
      setIsSaving(false);
    }
  }, [project, selectedFilePath, selectedFileContent, hasUnsavedChanges]);

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (justSavedTimerRef.current) {
        clearTimeout(justSavedTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (selectedFilePath) {
      document.title = `${hasUnsavedChanges ? "● " : ""}Editing ${selectedFilePath}`;
    }
  }, [selectedFilePath, hasUnsavedChanges]);

  return {
    selectedFilePath,
    selectedFileContent,
    setSelectedFileContent,
    previewUrl,
    fetchFileContent,
    saveFile,
    isSaving,
    hasUnsavedChanges,
    justSaved,
    error,
  };
};
