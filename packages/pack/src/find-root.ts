import findUp from "find-up";
import { dirname } from "path";

export function findRootLockFile(cwd: string) {
  return findUp.sync(
    [
      "pnpm-lock.yaml",
      "package-lock.json",
      "yarn.lock",
      "bun.lock",
      "bun.lockb",
    ],
    {
      cwd,
    },
  );
}

export function findRootDir(cwd: string): string {
  const lockFile = findRootLockFile(cwd);
  if (!lockFile) return cwd;

  const lockFiles = [lockFile];
  while (true) {
    const lastLockFile = lockFiles[lockFiles.length - 1];
    const currentDir = dirname(lastLockFile);
    const parentDir = dirname(currentDir);

    // dirname('/')==='/' so if we happen to reach the FS root (as might happen in a container we need to quit to avoid looping forever
    if (parentDir === currentDir) break;

    const newLockFile = findRootLockFile(parentDir);

    if (!newLockFile) break;

    lockFiles.push(newLockFile);
  }

  return dirname(lockFiles[lockFiles.length - 1]);
}
