const webpack = require("webpack");
const HtmlWebpackPlugin = require("html-webpack-plugin");

module.exports = {
  mode: process.env.NODE_ENV === "production" ? "production" : "development",
  entry: "./src/index.tsx",
  output: {
    path: "dist",
    filename: "[name].[contenthash:8].js",
    chunkFilename: "[name].[contenthash:8].js",
    cssFilename: "[name].[contenthash:8].css",
    assetModuleFilename: "[name].[contenthash:8].[ext]",
  },
  resolve: {
    alias: {
      "@": "./src",
    },
    extensions: [".js", ".jsx", ".ts", ".tsx"],
  },
  externals: {
    react: "React",
    "react-dom": "ReactDOM",
  },
  plugins: [
    new webpack.DefinePlugin({
      VERSION: JSON.stringify("5fa3b9"),
      BROWSER_SUPPORTS_HTML5: true,
      TWO: "1",
      "typeof window": JSON.stringify("object"),
      "process.env.NODE_ENV": JSON.stringify(
        process.env.NODE_ENV || "development",
      ),
    }),
    new HtmlWebpackPlugin({
      title: "Utoo Webpack Compat Demo",
      template: "./public/index.html",
    }),
  ],
  module: {
    rules: [
      {
        test: /\.html$/i,
        use: ["html-loader"],
      },
    ],
  },
  optimization: {
    moduleIds: "deterministic",
    minimize: process.env.NODE_ENV === "production",
    usedExports: true,
  },
};
