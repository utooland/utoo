import fs from "fs";
import path from "path";
import { ConfigComplete } from "../config/types";

/**
 * Validates that all entry.import paths exist in the filesystem.
 * Throws an error if any entry path doesn't exist.
 *
 * @param config - The complete config containing entry points
 * @param projectPath - The project root path to resolve relative entry paths
 * @throws {Error} If any entry.import path doesn't exist
 */
export function validateEntryPaths(
  config: ConfigComplete,
  projectPath: string,
): void {
  if (!config.entry || config.entry.length === 0) {
    return;
  }

  const errors: string[] = [];

  for (const entry of config.entry) {
    if (!entry.import) {
      errors.push(`Entry is missing 'import' property`);
      continue;
    }

    // Resolve the entry path relative to projectPath
    const entryPath = path.isAbsolute(entry.import)
      ? entry.import
      : path.resolve(projectPath, entry.import);

    // Check if the file exists
    if (!fs.existsSync(entryPath)) {
      const entryName = entry.name ? ` (name: "${entry.name}")` : "";
      errors.push(
        `Entry import path does not exist: "${entry.import}"${entryName}\n` +
          `  Resolved path: ${entryPath}`,
      );
    }
  }

  if (errors.length > 0) {
    throw new Error(
      `Invalid entry configuration:\n${errors.map((e) => `  - ${e}`).join("\n")}`,
    );
  }
}
