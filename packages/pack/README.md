# @utoo/pack

The high-performance bundler core for the Utoo toolchain, built on top of [Turbopack](https://turbo.build/pack).

## ✨ Features

- 🚀 **Extreme Performance**: Core logic implemented in Rust via NAPI-RS.
- 🛠️ **Turbopack Powered**: Leverages Turbopack's incremental computation engine.
- 📦 **Modern Web Support**: Built-in support for TypeScript, JSX, CSS Modules, and more.
- 🔧 **Extensible**: Support for custom loaders and plugins.

## 📦 Installation

```bash
npm install @utoo/pack
```

## 🚀 Usage

### Programmatic API

```javascript
const { build, dev } = require('@utoo/pack');

// Production build
await build({
  root: process.cwd(),
  entry: {
    main: './src/index.ts'
  },
  // ... other options
});

// Development mode with HMR
const server = await dev({
  root: process.cwd(),
  // ... other options
});
```

## 🛠️ Development

### Prerequisites

- Rust toolchain (nightly)
- Node.js 20+

### Build from source

```bash
# Build Rust bindings and TypeScript
npm run build
```

## 📄 License

[MIT](./LICENSE)
