import { Project as UtooProject } from "@utoo/web";
import JSZip from "jszip";
import { useCallback, useRef, useState } from "react";

export const useUpload = (
  project: UtooProject | null,
  onSuccess?: () => void,
) => {
  const [isUploading, setIsUploading] = useState(false);
  const [error, setError] = useState<string>("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  const clearProjectFiles = async (proj: UtooProject): Promise<void> => {
    const items = await proj.readdir(".");
    for (const item of items) {
      const name = item.name;
      // Skip node_modules and package-lock.json to preserve installed dependencies
      if (name === "node_modules" || name === "package-lock.json") continue;
      try {
        if (item.isDirectory()) {
          await proj.rmdir(name, { recursive: true });
        } else {
          await proj.rm(name);
        }
      } catch (e) {
        console.warn(`Failed to delete ${name}:`, e);
      }
    }
  };

  const extractZip = async (
    proj: UtooProject,
    file: File,
  ): Promise<void> => {
    const zip = await JSZip.loadAsync(file);
    const createdDirs = new Set<string>();

    // Find common prefix (if zip has a single root folder)
    const paths = Object.keys(zip.files).filter((p) => !zip.files[p].dir);
    let prefix = "";
    if (paths.length > 0) {
      const firstPath = paths[0];
      const slashIndex = firstPath.indexOf("/");
      if (slashIndex !== -1) {
        const potentialPrefix = firstPath.substring(0, slashIndex + 1);
        const allHavePrefix = paths.every((p) => p.startsWith(potentialPrefix));
        if (allHavePrefix) {
          prefix = potentialPrefix;
        }
      }
    }

    for (const [relativePath, zipEntry] of Object.entries(zip.files)) {
      if (zipEntry.dir) continue;

      // Remove common prefix if exists
      let filePath = relativePath;
      if (prefix && filePath.startsWith(prefix)) {
        filePath = filePath.substring(prefix.length);
      }
      if (!filePath) continue;

      // Skip package-lock.json from zip
      if (filePath === "package-lock.json") continue;

      // Create parent directories if needed
      const lastSlashIndex = filePath.lastIndexOf("/");
      if (lastSlashIndex !== -1) {
        const dirPath = filePath.substring(0, lastSlashIndex);
        if (!createdDirs.has(dirPath)) {
          await proj.mkdir(dirPath, { recursive: true });
          createdDirs.add(dirPath);
        }
      }

      // Determine if file is binary or text
      const ext = filePath.split(".").pop()?.toLowerCase() || "";
      const binaryExts = [
        "png", "jpg", "jpeg", "gif", "webp", "ico", "svg",
        "woff", "woff2", "ttf", "eot", "otf",
        "zip", "tar", "gz", "7z",
        "pdf", "doc", "docx",
        "mp3", "mp4", "wav", "webm",
      ];

      if (binaryExts.includes(ext)) {
        const content = await zipEntry.async("uint8array");
        await proj.writeFile(filePath, content);
      } else {
        const content = await zipEntry.async("string");
        await proj.writeFile(filePath, content);
      }
    }
  };

  const handleUpload = useCallback(async () => {
    if (!project) {
      setError("Project not initialized");
      return;
    }

    // Create and trigger file input
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".zip";
    inputRef.current = input;

    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;

      if (!file.name.endsWith(".zip")) {
        setError("Please select a .zip file");
        return;
      }

      setIsUploading(true);
      setError("");

      try {
        // Clear existing project files (except node_modules and package-lock.json)
        await clearProjectFiles(project);

        // Extract zip contents
        await extractZip(project, file);

        console.log("Upload and extraction complete");
        onSuccess?.();
      } catch (e: any) {
        console.error("Upload failed:", e);
        setError(`Upload failed: ${e.message}`);
      } finally {
        setIsUploading(false);
      }
    };

    input.click();
  }, [project, onSuccess]);

  return {
    isUploading,
    handleUpload,
    error,
  };
};
