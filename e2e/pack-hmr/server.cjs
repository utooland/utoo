const path = require("node:path");
const { serve } = require("../../packages/pack/cjs");

const port = Number(process.argv[2] || 4199);
const projectPath = __dirname;

void serve(
  {
    config: {
      entry: [
        {
          import: "./src/index.js",
          name: "main",
          html: { template: "./index.html" },
        },
      ],
      output: { path: "./dist", clean: true },
      optimization: {
        minify: false,
        splitChunks: {
          js: {
            minChunkSize: 1,
            maxChunkCountPerGroup: 10,
            maxMergeChunkSize: 1,
          },
        },
      },
      persistentCaching: false,
    },
  },
  projectPath,
  projectPath,
  {
    hostname: "127.0.0.1",
    logServerInfo: false,
    port,
  },
);
