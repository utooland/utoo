module.exports = {
  entry: [
    {
      import: "./src/index.ts",
      html: {
        template: "./index.html",
      },
    },
  ],
  optimization: {
    minify: false,
  },
  styles: {
    less: {
      // Exercise v11 even when the workspace hoists a different less-loader.
      loader: require.resolve("less-loader"),
      javascriptEnabled: true,
    },
  },
  output: {
    path: "./dist",
    filename: "[name].[contenthash:8]",
    chunkFilename: "[name].[contenthash:8]",
    clean: true,
  },
  stats: true,
};
