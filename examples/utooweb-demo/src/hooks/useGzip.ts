import { PackFile, Project as UtooProject } from "@utoo/web";
import { useCallback, useState } from "react";

// Recursively collect all files from a directory with concurrent processing
async function collectFilesRecursively(
  project: UtooProject,
  dirPath: string,
  relativePath: string = "",
): Promise<PackFile[]> {
  const entries = await project.readdir(dirPath);

  // Process all entries concurrently
  const results = await Promise.all(
    entries.map(async (entry) => {
      const entryRelativePath = relativePath
        ? `${relativePath}/${entry.name}`
        : entry.name;
      const entryFullPath = `${dirPath}/${entry.name}`;

      if (entry.isDirectory()) {
        // Recursively collect files from subdirectory
        return collectFilesRecursively(project, entryFullPath, entryRelativePath);
      } else if (entry.isFile()) {
        try {
          const content = await project.readFile(entryFullPath);
          console.log(`Adding file: ${entryRelativePath} (${content.length} bytes)`);
          return [{ path: entryRelativePath, content }];
        } catch (e) {
          console.error(`Failed to read file ${entryFullPath}:`, e);
          return [];
        }
      }
      return [];
    }),
  );

  // Flatten results
  return results.flat();
}

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
      const files = await collectFilesRecursively(project, "dist");

      if (files.length === 0) {
        setError("No files found in dist directory");
        return;
      }

      // Calculate MD5 for each file concurrently
      console.time("files md5");
      const fileMd5s = await Promise.all(
        files.map(async (file) => {
          const md5 = await project.sigMd5(file.content);
          return { path: file.path, md5 };
        }),
      );
      console.timeEnd("files md5");

      // Build files MD5 map
      const filesMap: Record<string, string> = {};
      for (const { path, md5 } of fileMd5s) {
        filesMap[path] = `md5:${md5}`;
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
        files: filesMap,
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
