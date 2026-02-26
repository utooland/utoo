/**
 * Example: build using @utoo/pack ESM JS API (no utoopack.json / CLI).
 */
import { build } from "@utoo/pack";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectPath = __dirname;

const options = {
  config: {
    entry: [
      {
        import: "./src/index.ts",
        html: { template: "./index.html" },
      },
    ],
    output: {
      path: "./dist",
      filename: "[name].[contenthash:6].js",
      chunkFilename: "[name].[contenthash:8].js",
      clean: true,
    },
    sourceMaps: false,
    optimization: { minify: false },
  },
};

await build(options, projectPath);
console.log("Build done: dist/");
