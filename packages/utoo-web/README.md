# @utoo/web

> 🌖 Web-compatible version of the Utoo toolchain, powered by [Turbopack](https://turbo.build/pack) and WebAssembly.

`@utoo/web` brings the power of the Utoo bundler to the browser, leveraging WebAssembly to provide high-performance bundling in non-native environments. It enables a complete web development environment, including a virtual file system and dependency management, entirely within the browser.

## ✨ Features

- 🌐 **Browser-based Bundling**: Run the Utoo bundler directly in the browser.
- ⚡ **WASM Powered**: High performance via WebAssembly bindings of the Turbopack-based core.
- 🛠️ **Web Worker Support**: Offloads heavy bundling tasks to background workers to keep the UI responsive.
- 🔌 **Webpack Compatibility**: Supports a subset of Webpack configurations in the browser.
- 📂 **Real File System**: Uses Origin Private File System (OPFS) for a Node.js-like file system experience.

## 🧠 Core Concepts

1. **Real File System**: The project lives in the browser's OPFS.
2. **Project Main Worker**: Core logic runs in a dedicated worker to keep the UI responsive.
3. **Thread Worker**: Heavy bundling tasks are offloaded to a worker powered by a ported `tokio` runtime.
4. **Loader Worker**: Executes Webpack loaders in a dedicated worker with Node.js polyfills.
5. **Service Worker**: Acts as a local server to intercept requests and serve built files for preview.

## 📦 Installation

```bash
npm install @utoo/web
```

## 🛠️ Development

### Prerequisites

- Rust toolchain (nightly)
- `wasm-bindgen-cli`
- `binaryen` (for `wasm-opt`)

### Build

```bash
# Install toolchain
npm run install-toolchain

# Build WASM and TypeScript
npm run build
```

### Run Demo

To run the web demo:

```bash
npm start -w utooweb-demo
```

## 📚 Documentation

For detailed API usage and examples, please refer to the [API Documentation](./API.md) ([中文版](./API_zh-CN.md)).

## �📄 License

[MIT](./LICENSE)
