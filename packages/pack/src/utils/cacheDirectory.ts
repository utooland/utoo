import path from "path";

export const DEFAULT_CACHE_DIRECTORY = ".turbopack";

/**
 * Resolves the directory that holds the persistent cache, lock file and
 * traces. Relative values resolve from the project path.
 */
export function resolveCacheDirectory(
  projectPath: string,
  cacheDirectory: string | undefined,
): string {
  return path.resolve(projectPath, cacheDirectory || DEFAULT_CACHE_DIRECTORY);
}
