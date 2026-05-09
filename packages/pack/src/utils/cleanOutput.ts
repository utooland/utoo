import fs from "fs";
import path from "path";
import type { ConfigComplete } from "../config/types";

export function getOutputPath(config: ConfigComplete, projectPath: string) {
  return path.resolve(projectPath, config.output?.path || "dist");
}

export async function cleanOutput(config: ConfigComplete, projectPath: string) {
  if (!config.output?.clean) {
    return;
  }

  const outputPath = getOutputPath(config, projectPath);
  let entries: fs.Dirent[];

  try {
    entries = await fs.promises.readdir(outputPath, { withFileTypes: true });
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      return;
    }
    throw error;
  }

  await Promise.all(
    entries.map((entry) =>
      fs.promises.rm(path.join(outputPath, entry.name), {
        force: true,
        maxRetries: 3,
        recursive: true,
        retryDelay: 50,
      }),
    ),
  );
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}
