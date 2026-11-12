import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

export default {
  server: { middlewareMode: true },
  resolve: {
    alias: [
      {
        find: "../core/hmr",
        replacement: fileURLToPath(import.meta.url),
      },
    ],
  },
};

// Keep the real dev server and signal handlers; control only its async flush.
export async function createHotReloader() {
  let closeCalls = 0;
  return {
    start: async () => {},
    getClientPaths: () => [],
    registerClient: () => {},
    unregisterClient: () => {},
    handleClientMessage: () => {},
    close: async () => {
      closeCalls++;
      await new Promise<void>((resolve) => {
        const onMessage = (message: unknown) => {
          if (message !== "release") return;
          process.off("message", onMessage);
          resolve();
        };
        process.on("message", onMessage);
        process.send?.("closing");
      });
      const directory = process.env.UTOO_SIGNAL_TEST_DIR;
      if (!directory) throw new Error("Missing signal fixture directory");
      fs.writeFileSync(
        path.join(directory, "flushed.json"),
        JSON.stringify({ closeCalls }),
      );
      if (process.env.UTOO_SIGNAL_TEST_FAIL === "1") {
        throw new Error("Cache persistence failed");
      }
    },
  };
}
