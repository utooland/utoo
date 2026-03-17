/**
 * Utoopack build configuration.
 * Passed directly to build() and dev() instead of reading from utoopack.json on disk.
 */
export const utoopackConfig = {
  entry: [
    {
      import: "./src/index.tsx",
      name: "index",
    },
  ],
  output: {
    publicPath: "/preview/dist",
  },
  nodePolyFill: true,
  stats: true,
};
