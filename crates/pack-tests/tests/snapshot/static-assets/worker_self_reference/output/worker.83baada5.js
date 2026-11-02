export function spawnWorker() {
  return new Worker(new URL("./worker.js", import.meta.url), {
    type: "module",
  });
}

self.onmessage = () => {
  postMessage("self-referencing worker");
};

globalThis.spawnWorker = spawnWorker;