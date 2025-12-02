const path = require("path");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const MiniCssExtractPlugin = require("mini-css-extract-plugin");
const { DefinePlugin } = require("webpack");

module.exports = [
  {
    mode: "development",
    entry: {
      main: {
        import: "./src/index.tsx",
        filename: "main.js",
      },
    },
    devtool: "source-map",
    output: {
      path: path.resolve(__dirname, "dist"),
      clean: false,
    },
    module: {
      rules: [
        {
          test: /demo_raw\/.*/,
          type: "asset/source",
        },
        {
          test: /\.tsx?$/,
          exclude: /(node_modules)|(demo_raw\/.*)/,
          use: {
            loader: "ts-loader",
            options: {
              transpileOnly: true,
            },
          },
        },
        {
          test: /\.css$/,
          exclude: /demo_raw\/.*/,
          use: [MiniCssExtractPlugin.loader, "css-loader"],
        },
      ],
    },
    resolve: {
      extensions: [".tsx", ".ts", ".js"],
    },
    plugins: [
      new DefinePlugin({
        process: {},
        "process.env": JSON.stringify({
          NODE_ENV: process.env.NODE_ENV || "production",
        }),
      }),
      new HtmlWebpackPlugin({
        template: "./index.html",
        title: "Utooweb demo",
      }),
      new MiniCssExtractPlugin({
        filename: "[name].css",
      }),
    ],
    devServer: {
      devMiddleware: {
        writeToDisk: true,
      },
      headers: {
        "Access-Control-Allow-Origin": "*",
        "Cross-Origin-Opener-Policy": "same-origin",
        "Cross-Origin-Embedder-Policy": "require-corp",
      },
      hot: false,
      liveReload: false,
      client: false,
      webSocketServer: false,
      port: 8081,
    },
  },
  {
    mode: "development",
    entry: {
      worker: {
        import: "./src/worker.ts",
        filename: "worker.js",
        chunkLoading: false,
      },
      tokioWorker: {
        import: "./src/threadWorker.ts",
        filename: "threadWorker.js",
        chunkLoading: false,
      },
      serviceWorker: {
        import: "./src/serviceWorker.ts",
        filename: "serviceWorker.js",
        chunkLoading: false,
      },
    },
    devtool: "cheap-module-source-map",
    output: {
      path: path.resolve(__dirname, "dist"),
      clean: false,
    },
    module: {
      rules: [
        {
          test: /\.ts?$/,
          exclude: /node_modules/,
          use: {
            loader: "ts-loader",
            options: {
              transpileOnly: true,
            },
          },
        },
      ],
    },
    resolve: {
      extensions: [".ts", ".js"],
    },
  },
];
