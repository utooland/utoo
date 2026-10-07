import "./style.css";
import { increment, live } from "./shared.js";

export { executions, live } from "./shared.js";
export { value } from "./barrel.js";
export const entry = "sync-a";

export function bump() {
  increment();
  return live;
}

export async function load() {
  return (await import("./dynamic.js")).default;
}

export function startWorker() {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./worker.js", import.meta.url));
    worker.onmessage = (event) => {
      worker.terminate();
      resolve(event.data);
    };
    worker.onerror = reject;
    worker.postMessage("read");
  });
}

export default { entry };
