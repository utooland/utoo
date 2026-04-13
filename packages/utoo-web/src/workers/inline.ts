/**
 * Creates a Worker from a data URI string, using Blob URL for cross-browser
 * compatibility (Firefox does not support `new Worker("data:...")` directly).
 */
export function createWorkerFromDataUri(
  dataUri: string,
  options?: WorkerOptions,
): Worker {
  const base64Marker = ";base64,";
  const idx = dataUri.indexOf(base64Marker);
  if (idx === -1) {
    // Not base64 encoded, try directly (unlikely path)
    return new Worker(dataUri, options);
  }
  const base64 = dataUri.slice(idx + base64Marker.length);
  const code = atob(base64);
  const blob = new Blob([code], { type: "application/javascript" });
  return new Worker(URL.createObjectURL(blob), options);
}
