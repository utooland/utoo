> 🌖 **Notice**: We are building a next-generation toolchain on top of [Turbopack](https://turbo.build/pack). Check out our [progress](https://github.com/utooland/utoo/issues/1872).

<div align="center">
<img src="https://mdn.alipayobjects.com/huamei_botco4/afts/img/357RTIva8S8AAAAAAAAAAAAADnNMAQFr/original" alt="Utoo Logo" width="120"/>
<h1>Utoo</h1>
<p><strong>Unified Toolchain: Open & Optimized</strong></p>

[![License](https://img.shields.io/github/license/utooland/utoo?style=flat-square)](./LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/utooland/utoo/ci.yml?style=flat-square)](https://github.com/utooland/utoo/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](./CONTRIBUTING.md)

</div>

---

Utoo is a modern, high-performance frontend toolchain designed to provide a unified and optimized experience. It combines a fast package manager, a powerful bundler, and a flexible command system into a single, cohesive ecosystem.

## 💡 Why Utoo?

| Feature | Description |
| :--- | :--- |
| **Unified** | One tool for package management, building, and workflow automation. |
| **Performance** | Core logic in Rust + Turbopack for extreme speed. |
| **Compatible** | Seamless migration with Webpack compatibility mode. |
| **Universal** | Run anywhere: Local, CI, or Browser (via WASM). |

## 📦 Core Components

- **`utoo`**: High-performance Rust package manager (Fast, Parallel, `npm` compatible).
- **`@utoo/pack`**: Next-gen bundler powered by **Turbopack** (HMR, TS/JSX, Less/Sass).
- **`ut`**: Command mounting system to unify your local and global workflows.

## 🚀 Quick Start

### 1. Install

```bash
# Install the core toolchain
npm install -g utoo

# Install the bundler (optional)
npm install -g @utoo/pack
```

### 2. Use

#### Package Management
```bash
utoo install          # Install dependencies
utoo add lodash       # Add a package
utoo x create-react   # Execute a package (npx style)
```

#### Bundling
```bash
utoopack dev          # Start dev server with HMR
utoopack build        # Production build
utoopack build --webpack # Build using webpack.config.js
```

## ✨ Key Features

- ⚡ **Rust Powered**: Maximum performance for dependency resolution and bundling.
- 🛠️ **Turbopack Inside**: Incremental builds and instant HMR.
- 🔌 **Webpack Friendly**: Partial support for existing Webpack configurations.
- 📦 **Monorepo First**: Built-in workspace management.
- 🌐 **Web Ready**: Run the entire toolchain in the browser via WASM.

## 🗺️ Roadmap

We are actively working on expanding Utoo's capabilities:

- [ ] **Persistent Caching**: Disk-based caching for even faster restarts.
- [ ] **SSR & RSC**: Full support for modern React architectures.
- [ ] **Plugin System**: Extensible hooks for custom build logic.
- [ ] **Mobile Support**: Unified tooling for mobile frontend development.

## �📂 Project Structure

- **[crates/](crates/)**: Rust core (Package Manager, Bundler Core, WASM/NAPI bindings).
- **[packages/](packages/)**: TypeScript packages (CLI, Web version, Shared utilities).
- **[next.js/](next.js/)**: Turbopack source integration (Git submodule).
- **[examples/](examples/)**: Demo projects (React, Ant Design, Webpack-compat, etc.).

## 🤝 Contributing

We love contributions! Check out [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## 📄 License

Utoo is licensed under the [MIT License](LICENSE).

