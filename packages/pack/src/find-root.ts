import findUp from "find-up";
import { readFileSync } from "fs";
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

// compitable with tnpm
export function findPackageJson(cwd: string) {
  return findUp.sync(["package.json"], {
    cwd,
  });
}

function isWorkspaceRoot(pkgPath: string) {
  const pkgJson = readFileSync(pkgPath, "utf-8");
  const pkgJsonContent = JSON.parse(pkgJson);
  return Boolean(pkgJsonContent.workspaces);
}

// refer from: https://github.com/umijs/mako/blob/next/crates/pm/src/helper/workspace.rs#L153
// TODO: 这块逻辑后续跟 utoo-pkg 使用一套方法
export function findWorkspacesRoot(cwd: string) {
  const pkgJson = findPackageJson(cwd);
  if (!pkgJson) return cwd;
  const pkgJsonFiles = [pkgJson];
  while (true) {
    const lastPkgJson = pkgJsonFiles[pkgJsonFiles.length - 1];

    const currentDir = dirname(lastPkgJson);
    const parentDir = dirname(currentDir);

    if (parentDir === currentDir) break;

    if (isWorkspaceRoot(lastPkgJson)) break;

    const newPkgJson = findPackageJson(parentDir);
    if (!newPkgJson) break;

    pkgJsonFiles.push(newPkgJson);
  }

  return dirname(pkgJsonFiles[pkgJsonFiles.length - 1]);
}

export function findRootDir(cwd: string): string {
  const lockFile = findRootLockFile(cwd);
  if (!lockFile) return findWorkspacesRoot(cwd);

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
