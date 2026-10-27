import { getRootPathFromEnv } from "./env";
import { findRootDir } from "./findRoot";

export function resolveRootPath(
  projectPath?: string,
  rootPath?: string,
): string {
  return (
    getRootPathFromEnv() ||
    rootPath ||
    findRootDir(projectPath || process.cwd())
  );
}
