// @ts-check
import { defineConfig } from "@utoo/pack";

/** Example JS config: same shape as utoopack.json, can use dynamic values. */
export default defineConfig({
  define: {
    GREETING: '"Hello from JS config"',
    APP_VERSION: "1",
  },
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
});
