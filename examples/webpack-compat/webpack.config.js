const webpack = require("webpack");
const HtmlWebpackPlugin = require("html-webpack-plugin");

module.exports = {
  mode: process.env.NODE_ENV === "production" ? "production" : "development",
  entry: "./src/index.js",
  output: {
    path: "dist",
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
      "process.env.NODE_ENV": JSON.stringify(process.env.NODE_ENV),
    }),
    new HtmlWebpackPlugin(),
  ],
  module: {
    rules: [
      {
        test: /\.html$/i,
        use: ["html-loader"],
        options: {
          esModule: true,
        },
      },
    ],
  },
  optimization: {
    moduleIds: "named",
    minimize: false,
  },
};
