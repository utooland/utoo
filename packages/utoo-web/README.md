# @utoo/web

> 🌖 Web-compatible version of the Utoo toolchain, powered by [Turbopack](https://turbo.build/pack) and WebAssembly.

`@utoo/web` brings the power of the Utoo bundler to the browser, leveraging WebAssembly to provide high-performance bundling in non-native environments. It enables a complete web development environment, including a real file system and dependency management, entirely within the browser.

## ✨ Features

- 📂 **Real File System**: Uses Origin Private File System (OPFS) for a Node.js-like file system experience.
- 📦 **Browser Dependency Resolution**: Resolve dependencies directly from `package.json` without needing a pre-existing lock file. Supports custom registries (npm, npmmirror, private registries).
- 🌐 **Browser-based Bundling**: Run the Utoo bundler directly in the browser.
- ♻️ **Project Lifecycle**: Cancel active installs or builds with `dispose()` and safely switch to another project.
- � **Hot Module Replacement (HMR)**: Full HMR support in the browser via MessagePort communication with preview iframes.
- �🔌 **Webpack Compatibility**: Supports a subset of Webpack configurations in the browser. See [Features List](../pack/docs/features-list.md) for details.
- 🔄 **Webpack Loaders**: Compatible with standard Webpack loaders (css-loader, style-loader, etc.).
- 🎨 **PostCSS & Tailwind**: Support for PostCSS and Tailwind CSS processing.


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

For detailed API usage and examples, please refer to the [API Documentation](./API.md) ([中文版](./API_zh-CN.md)). For project switching, see [Switching Projects During Install or Build](./API.md#switching-projects-during-install-or-build) ([中文说明](./API_zh-CN.md#在安装或构建期间切换项目)).

## �📄 License

[MIT](./LICENSE)
