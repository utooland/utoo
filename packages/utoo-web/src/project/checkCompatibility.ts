export function checkCompatibility(): {
  compatible: boolean;
  minimalVersion: { chrome: number };
  details: {
    sharedArrayBuffer: boolean;
    atomics: boolean;
    worker: boolean;
    serviceWorker: boolean;
    opfs: boolean;
    fileSystemObserver: boolean;
  };
  missing: string[];
} {
  const details = {
    sharedArrayBuffer:
      typeof SharedArrayBuffer !== "undefined" && crossOriginIsolated,
    atomics: typeof Atomics !== "undefined",
    worker: typeof Worker !== "undefined",
    serviceWorker: "serviceWorker" in navigator,
    opfs: "storage" in navigator && "getDirectory" in navigator.storage,
    fileSystemObserver: "FileSystemObserver" in window,
  };

  const missing: string[] = [];

  if (!details.sharedArrayBuffer) missing.push("SharedArrayBuffer (COOP/COEP)");
  if (!details.atomics) missing.push("Atomics");
  if (!details.worker) missing.push("Worker");
  if (!details.serviceWorker) missing.push("ServiceWorker");
  if (!details.opfs) missing.push("OPFS");
  if (!details.fileSystemObserver) missing.push("FileSystemObserver");

  const compatible = missing.length === 0;
  const minimalVersion = { chrome: 137 };

  return {
    compatible,
    minimalVersion,
    details,
    missing,
  };
}
