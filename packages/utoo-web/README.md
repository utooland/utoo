# @utoo/web

Web-compatible version of the Utoo toolchain, powered by WebAssembly.

## ✨ Features

- 🌐 **Browser-based Bundling**: Run the Utoo bundler directly in the browser.
- ⚡ **WASM Powered**: High performance via WebAssembly bindings.
- 🛠️ **Web Worker Support**: Offloads heavy bundling tasks to background workers.

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

## 📄 License

[MIT](./LICENSE)
