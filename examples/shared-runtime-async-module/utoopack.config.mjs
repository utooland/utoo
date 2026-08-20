import { fileURLToPath } from "node:url";
import { defineConfig } from "@utoo/pack";

export default defineConfig({
  mode: "development",
  entry: [
    {
      name: "main",
      import: "./src/index.js",
    },
  ],
  module: {
    rules: {
      "*.sync-txt": {
        loaders: [
          fileURLToPath(new URL("./sync-loader.cjs", import.meta.url)),
        ],
        as: "*.js",
      },
    },
  },
  output: {
    clean: true,
  },
  pluginRuntimeStrategy: "workerThreads",
  sourceMaps: true,
});
