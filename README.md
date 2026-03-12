<div align="center">
<img src="https://mdn.alipayobjects.com/huamei_botco4/afts/img/357RTIva8S8AAAAAAAAAAAAADnNMAQFr/original" alt="Utoo Logo" height="80"/>
<h1>Utoo</h1>
<p><strong>Unified Toolchain: Open & Optimized</strong></p>

[![license](https://img.shields.io/github/license/utooland/utoo?style=flat-square)](./LICENSE)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](./CONTRIBUTING.md)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/utooland/utoo)

[![npm](https://img.shields.io/npm/v/utoo?style=flat-square&label=utoo)](https://www.npmjs.com/package/utoo)
[![npm](https://img.shields.io/npm/v/@utoo/pack?style=flat-square&label=@utoo/pack)](https://www.npmjs.com/package/@utoo/pack)
[![npm](https://img.shields.io/npm/v/@utoo/web?style=flat-square&label=@utoo/web)](https://www.npmjs.com/package/@utoo/web)

[![downloads](https://img.shields.io/npm/dm/utoo?style=flat-square&label=utoo%20downloads)](https://www.npmjs.com/package/utoo)
[![downloads](https://img.shields.io/npm/dm/@utoo/pack?style=flat-square&label=@utoo/pack%20downloads)](https://www.npmjs.com/package/@utoo/pack)
[![downloads](https://img.shields.io/npm/dm/@utoo/web?style=flat-square&label=@utoo/web%20downloads)](https://www.npmjs.com/package/@utoo/web)

[![PM CI](https://img.shields.io/github/actions/workflow/status/utooland/utoo/pm-ci.yml?style=flat-square&label=PM%20CI)](https://github.com/utooland/utoo/actions/workflows/pm-ci.yml)
[![Pack CI](https://img.shields.io/github/actions/workflow/status/utooland/utoo/pack-ci.yml?style=flat-square&label=Pack%20CI)](https://github.com/utooland/utoo/actions/workflows/pack-ci.yml)

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

- **[`utoo`](crates/pm)** (alias **`ut`**): High-performance Rust package manager (Fast, Parallel, `npm` compatible).
- **[`@utoo/pack`](packages/pack)**: Next-gen bundler powered by **Turbopack** (HMR, TS/JSX, Less/Sass, and more).
- **[`@utoo/pack-cli`](packages/pack-cli)**: Command-line interface for the bundler (dev server, build). A lightweight wrapper to run `up` commands — `up` is a shortcut alias for `utoopack`.
- **[`@utoo/web`](packages/utoo-web)**: Web-compatible version of the toolchain (WASM, Browser-based bundling).

## 🚀 Quick Start

### 1. Install

```bash
# Install the core toolchain
npm install -g utoo

# Install the bundler in nodejs environment
ut install @utoo/pack --save-dev

# Install the bundler cli in nodejs environment(Optional)
ut install @utoo/pack-cli --save-dev

# Install the web version
ut install @utoo/web --save
```

### 2. Use

#### Package Management
```bash
ut install          # Install dependencies (or use `ut install`)
ut add lodash       # Add a package (or use `ut add`)
ut x create-react   # Execute a package (npx style, or use `ut x`)
```

#### Bundling via @utoo/pack-cli
```bash
utx up dev          # Start dev server with HMR
utx up build        # Production build
utx up build --webpack # Build using webpack.config.js
```

## ✨ Key Features

- ⚡ **Rust Powered**: Maximum performance for dependency resolution and bundling.
- 🛠️ **Turbopack Inside**: Incremental builds and instant HMR.
- 🔌 **Webpack Friendly**: Partial support for existing Webpack configurations.
- 📦 **Monorepo First**: Built-in workspace management.
- 🌐 **Web Ready**: Run the entire toolchain in the browser via WASM.

## 📂 Project Structure

- **[crates/](crates/)**: Rust core (Package Manager, Bundler Core, WASM/NAPI bindings).
- **[packages/](packages/)**: TypeScript packages (CLI, Web version, Shared utilities).
- **[next.js/](next.js/)**: Turbopack source integration (Git submodule).
- **[examples/](examples/)**: Demo projects (React, Ant Design, Webpack-compat, etc.).

## 🤝 Contributing

We love contributions! Check out [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

## 📄 License

Utoo is licensed under the [MIT License](LICENSE).

