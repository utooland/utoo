import semver from "semver";

export function useWorkerThreads(): boolean {
  const WORKER_THREADS_SAFE_RANGE = ">=24.13.1 <25.0.0 || >=25.4.0";
  return semver.satisfies(process.version, WORKER_THREADS_SAFE_RANGE, {
    includePrerelease: true,
  });
}
