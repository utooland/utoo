import { executions, live } from "./shared.js";

export const entry = "worker";

self.onmessage = async () => {
  self.postMessage({
    entry,
    executions,
    live,
    dynamic: (await import("./dynamic.js")).default,
  });
};
