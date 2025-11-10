import { useState, useCallback } from "react";
import { Project as UtooProject, PackFile } from "@utoo/web";

export const useGzipMainThread = (project: UtooProject | null) => {
  const [isGzipping, setIsGzipping] = useState(false);
  const [error, setError] = useState("");
  const [gzipSuccess, setGzipSuccess] = useState(false);

  const handleGzipMainThread = useCallback(async () => {
    if (!project) return;
    setIsGzipping(true);
    setError("");
    setGzipSuccess(false);

    try {
      console.log("=== [Main Thread Gzip] Starting ===");

      // Read all files from dist directory recursively
      const distFiles = await project.readdir("dist", { recursive: true });

      const files: PackFile[] = [];

      // Read content of each file
      for (const file of distFiles) {
        if (file.isFile()) {
          const fullPath = `dist/${file.name}`;
          try {
            const content = await project.readFile(fullPath);
            console.log(`[Main Thread] Adding file: ${file.name} (${content.length} bytes)`);
            files.push({
              path: file.name,
              content: content,
            });
          } catch (e) {
            console.error(`Failed to read file ${fullPath}:`, e);
          }
        }
      }

      if (files.length === 0) {
        setError("No files found in dist directory");
        return;
      }

      // Create temporary archive in main thread (pure WASM + OPFS)
      console.log("[Main Thread] Creating temporary archive directly in main thread...");
      try {
        await project.gzipMainThread(files, "dist_temp_main.tgz");
        console.log("[Main Thread] Temporary archive created successfully");
      } catch (e) {
        console.error("[Main Thread] Failed to create temp archive:", e);
        throw e;
      }

      console.log("[Main Thread] Reading archive from OPFS in main thread...");
      const tempArchiveContent = await project.readFile("dist_temp_main.tgz");
      console.log(`[Main Thread] Read ${tempArchiveContent.length} bytes from OPFS`);

      console.log("[Main Thread] Calculating MD5 directly via WASM (no worker overhead)...");
      const md5Hash = await project.sigMd5MainThread(tempArchiveContent);
      console.log(`[Main Thread] Archive MD5: ${md5Hash}`);

      // Create config.json with MD5 signature (in-memory only, not written to disk)
      const configContent = {
        "dist.tgz": `md5:${md5Hash}`,
        generatedAt: new Date().toISOString(),
        fileCount: files.length,
        method: "main-thread",
      };
      const configJson = JSON.stringify(configContent, null, 2);
      const configBytes = new TextEncoder().encode(configJson);

      console.log("[Main Thread] Adding in-memory config.json:", configJson);

      // Add config.json to the files array (in-memory only)
      files.push({
        path: "config.json",
        content: configBytes,
      });

      // Create final tar.gz archive with config.json included
      console.log("[Main Thread] Creating final archive with config.json...");
      await project.gzipMainThread(files, "dist_main.tgz");

      console.log(`[Main Thread] Successfully created dist_main.tgz with ${files.length} files (including config.json)`);
      setGzipSuccess(true);

      // Read the final archive for download
      const archiveContent = await project.readFile("dist_main.tgz");

      // Clean up temporary file
      try {
        await project.rm("dist_temp_main.tgz");
      } catch (e) {
        console.warn("Failed to clean up temp file:", e);
      }

      // Trigger browser download
      const blob = new Blob([archiveContent], { type: "application/x-gzip" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "dist_main.tgz";
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);

      console.log("[Main Thread] Download triggered successfully");
      console.log("=== [Main Thread Gzip] Completed ===");

    } catch (e: any) {
      console.error("[Main Thread] Gzip failed:", e);
      setError(`Main Thread Gzip failed: ${e.message || JSON.stringify(e)}`);
    } finally {
      setIsGzipping(false);
    }
  }, [project]);

  return { isGzipping, handleGzipMainThread, error, gzipSuccess };
};
