/**
 * Utoopack build configuration.
 * Passed directly to build() and dev() instead of reading from utoopack.json on disk.
 */
export const utoopackConfig = {
  output: {
    publicPath: "/preview/dist",
  },
  entry: [
    {
      import: "./src/index.tsx",
      name: "index",
    },
  ],
  nodePolyFill: true,
  stats: true,
};
