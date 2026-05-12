import fs from "fs";
import path from "path";
import pc from "picocolors";
import * as binding from "../binding";

const RETRY_DELAY_MS = 10;
const MAX_RETRY_MS = 1000;

type NativeLockfile = { __napiType: "Lockfile" };

export class PersistentCacheLock {
  private listener: NodeJS.ExitListener | undefined;
  private nativeLockfile: NativeLockfile | undefined;

  private constructor(nativeLockfile: NativeLockfile) {
    this.nativeLockfile = nativeLockfile;
    this.listener = () => this.unlockSync();
    process.on("exit", this.listener);
  }

  static tryAcquire(lockPath: string, content?: string) {
    const nativeLockfile = binding.lockfileTryAcquireSync(lockPath, content);
    return nativeLockfile ? new PersistentCacheLock(nativeLockfile) : undefined;
  }

  static async acquireWithRetries(lockPath: string, processName: string) {
    const content = JSON.stringify({
      pid: process.pid,
      processName,
      startedAt: Date.now(),
    });
    const startMs = Date.now();

    let lockfile: PersistentCacheLock | undefined;
    while (Date.now() - startMs < MAX_RETRY_MS) {
      lockfile = PersistentCacheLock.tryAcquire(lockPath, content);
      if (lockfile) {
        return lockfile;
      }
      await new Promise((resolve) => setTimeout(resolve, RETRY_DELAY_MS));
    }

    throw new Error(formatLockError(lockPath, processName));
  }

  async unlock() {
    const lockfile = this.nativeLockfile;
    this.nativeLockfile = undefined;
    if (this.listener) {
      process.off("exit", this.listener);
      this.listener = undefined;
    }
    if (lockfile) {
      await binding.lockfileUnlock(lockfile);
    }
  }

  unlockSync() {
    const lockfile = this.nativeLockfile;
    this.nativeLockfile = undefined;
    if (this.listener) {
      process.off("exit", this.listener);
      this.listener = undefined;
    }
    if (lockfile) {
      binding.lockfileUnlockSync(lockfile);
    }
  }
}

export async function acquirePersistentCacheLock(
  projectPath: string,
  processName: string,
  persistentCaching: boolean,
) {
  if (!persistentCaching) {
    return undefined;
  }

  const internalDir = path.join(path.resolve(projectPath), ".turbopack");
  fs.mkdirSync(internalDir, { recursive: true });
  return PersistentCacheLock.acquireWithRetries(
    path.join(internalDir, "lock"),
    processName,
  );
}

function formatLockError(lockPath: string, processName: string) {
  let owner = "";
  try {
    const data = JSON.parse(fs.readFileSync(lockPath, "utf-8")) as {
      pid?: number;
      processName?: string;
    };
    if (data.pid) {
      const killCommand =
        process.platform === "win32"
          ? `taskkill /PID ${data.pid} /F`
          : `kill ${data.pid}`;
      owner = `\nExisting process: ${data.processName || "unknown"} (PID ${data.pid}).\nStop it with ${pc.cyan(killCommand)} if it is stale.`;
    }
  } catch {
    // The lock holder might be a version that did not write metadata.
  }

  return `Unable to acquire ${processName} persistent cache lock at ${pc.cyan(
    lockPath,
  )}. Another utoo pack process may be using the same .turbopack cache.${owner}`;
}
