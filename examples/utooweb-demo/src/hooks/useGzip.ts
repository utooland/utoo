import { PackFile, Project as UtooProject } from "@utoo/web";
import { useCallback, useState } from "react";

export const useGzip = (project: UtooProject | null) => {
  const [isGzipping, setIsGzipping] = useState(false);
  const [error, setError] = useState("");
  const [gzipSuccess, setGzipSuccess] = useState(false);

  const handleGzip = useCallback(async () => {
    if (!project) return;
    setIsGzipping(true);
    setError("");
    setGzipSuccess(false);

    try {
      // Read all files from dist directory recursively
      const distFiles = await project.readdir("dist", { recursive: true });

      const files: PackFile[] = [];

      // Read content of each file
      for (const file of distFiles) {
        if (file.isFile()) {
          const fullPath = `dist/${file.name}`;
          try {
            const content = await project.readFile(fullPath);
            console.log(`Adding file: ${file.name} (${content.length} bytes)`);
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

      // First, create a temporary archive to calculate its MD5
      const tempArchiveContent = await project.gzip(files);
      const md5Hash = await project.sigMd5(tempArchiveContent);
      console.log(`Archive MD5: ${md5Hash}`);

      // Create config.json with MD5 signature (in-memory only, not written to disk)
      const configContent = {
        "dist.tgz": `md5:${md5Hash}`,
        generatedAt: new Date().toISOString(),
        fileCount: files.length,
      };
      const configJson = JSON.stringify(configContent, null, 2);
      const configBytes = new TextEncoder().encode(configJson);

      console.log("Adding in-memory config.json:", configJson);

      // Add config.json to the files array (in-memory only)
      files.push({
        path: "config.json",
        content: configBytes,
      });

      console.time("work gzip");
      const archiveContent = await project.gzip(files);

      console.timeEnd("work gzip");
      console.log(
        `Successfully created dist.tgz with ${files.length} files (including config.json)`,
      );
      setGzipSuccess(true);

      // Clean up temporary file
      try {
        await project.rm("dist_temp.tgz");
      } catch (e) {
        console.warn("Failed to clean up temp file:", e);
      }

      // Trigger browser download
      const uint8Array = new Uint8Array(archiveContent);
      const blob = new Blob([uint8Array], { type: "application/gzip" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "dist.tgz";
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);

      console.log("Download triggered successfully");
    } catch (e: any) {
      console.error("Gzip failed:", e);
      setError(`Gzip failed: ${e.message || JSON.stringify(e)}`);
    } finally {
      setIsGzipping(false);
    }
  }, [project]);

  return { isGzipping, handleGzip, error, gzipSuccess };
};
