# @utoo/pack

> 🌖 High-performance bundler core for the Utoo toolchain, powered by [Turbopack](https://turbo.build/pack).

`@utoo/pack` is the engine behind the Utoo build system. It leverages the incremental computation power of Turbopack and the performance of Rust to provide a lightning-fast development and build experience.

## ✨ Key Features

- ⚡ **Extreme Performance**: Core bundling logic implemented in Rust via NAPI-RS.
- 🛠️ **Turbopack Powered**: Built on top of the same engine that powers Next.js Turbopack.
- 🔌 **Webpack Compatibility**: Support for consuming `webpack.config.js` to simplify migration from Webpack.
- 📦 **Modern Web Support**: Native support for TypeScript, JSX, CSS Modules, Less, Sass, and more.
- 🔧 **Extensible Architecture**: Support for custom loaders, plugins, and flexible configuration.
- 🔄 **Fast HMR**: Instant updates during development with optimized Hot Module Replacement.

## ✨ Supported Features

`@utoo/pack` aims for high compatibility with the Webpack ecosystem while providing superior performance.

- **Entry**: Supports `name`, `import`, and `filename` templates.
- **Module Rules**: Support for most mainstream Webpack loaders via `loader-runner`.
- **Resolve**: Full support for `alias` and `extensions`.
- **Styles**: Built-in support for Less, Sass, PostCSS, CSS Modules, and LightningCSS.
- **Optimization**: Minification, Tree Shaking, Module Concatenation, and more.
- **Frameworks**: Optimized for React (including `styled-jsx`, `emotion`, `styled-components`).
- **Tools**: Integrated Bundle Analyzer and Tracing Logs.

> [!TIP]
> For a detailed status of all features, see the [Features List](./docs/features-list.md).

## 📦 Installation

```bash
ut install @utoo/pack --save-dev
```

## 🚀 Quick Start

### Programmatic API

You can use `@utoo/pack` directly in your Node.js scripts:

```javascript
const { build, dev } = require('@utoo/pack');

// Production build
async function runBuild() {
  await build({
    mode: "production",
    entry: [
      {
        import: "./src/index.ts",
        html: {
          template: "./index.html"
        }
      }
    ],
    output: {
      path: "./dist",
      filename: "[name].[contenthash:8].js",
      chunkFilename: "[name].[contenthash:8].js",
      clean: true
    },
    sourceMaps: true
  });
}

// Development mode with HMR
async function startDev() {
  const server = await dev({
    mode: "development",
    entry: [
      {
        import: "./src/index.ts",
        html: {
          template: "./index.html"
        }
      }
    ],
    output: {
      path: "./dist",
      filename: "[name].[contenthash:8].js",
      chunkFilename: "[name].[contenthash:8].js",
      clean: true
    },
    sourceMaps: true
  });
}
```

## 🔌 Webpack Compatibility Mode

`@utoo/pack` provides a partial compatibility layer for Webpack.

```javascript
const { build } = require('@utoo/pack');
const webpackConfig = require('./webpack.config.js');

async function run() {
  await build({ ...webpackConfig, webpackMode: true });
}
```

> [!NOTE]
> Not all Webpack features and plugins are supported. Check the [Features List](./docs/features-list.md) for details on supported configuration options.

## ⚙️ Configuration

The bundler can be configured via a `utoopack.json` or through the programmatic API. Key configuration areas include:

- **`entry`**: Define your application entry points.
- **`define`**: Build-time variable replacement.
- **`externals`**: Exclude specific dependencies from the bundle.
- **`mode`**: `development` or `production`.

For a full list of options, see the [Configuration Schema](./config_schema.json).

## 🛠️ Development

### Prerequisites

- **Rust**: Nightly toolchain (see [rust-toolchain.toml](../../rust-toolchain.toml)).
- **Node.js**: Version 20 or higher.

### Building from Source

```bash
# Build Rust bindings and TypeScript modules
npm run build
```

## 📄 License

[MIT](./LICENSE)
