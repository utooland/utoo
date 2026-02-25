# Contributing to Utoo

Thank you for your interest in contributing to Utoo! This document provides a guide for setting up your development environment and working on the project.

## 🏗️ Architecture Overview

Utoo is a unified toolchain composed of several key layers:

1.  **Core (Rust)**: The high-performance engine.
    *   `crates/pm`: The `utoo` package manager (alias `ut`).
    *   `crates/pack-core`: The bundler core, built on **Turbopack**.
2.  **Bindings**: Bridging Rust to other environments.
    *   `crates/pack-napi`: Node.js bindings for the bundler.
    *   `crates/utoo-wasm`: WebAssembly bindings for browser usage.
3.  **Packages (TypeScript)**: User-facing tools and APIs.
    *   `@utoo/pack`: The main bundler library.
    *   `@utoo/pack-cli`: The `utoopack` CLI.
    *   `@utoo/web`: The browser-compatible toolchain.

## 🛠️ Prerequisites

Before you begin, ensure you have the following installed:

- **Node.js**: Version 20 or higher.
- **npm**: Version 10 or higher (managed via `packageManager` in `package.json`).
- **Rust**: We use a specific nightly version defined in [rust-toolchain.toml](rust-toolchain.toml). It will be automatically installed when you run Cargo commands.
- **wasm-bindgen-cli**: Required for building the web version.
  ```bash
  cargo install wasm-bindgen-cli@0.2.106
  ```
- **taplo**: Formats TOML files (e.g. `Cargo.toml`). The pre-push hook runs `taplo format --check`. Install: `cargo install taplo-cli --locked` or `brew install taplo`. Format manually: `RUST_LOG=warn taplo format`.
- **typos**: Spell checker for source and docs. The pre-push hook runs `typos`. Install: `cargo install typos-cli --locked` or `brew install typos-cli`. Config: [.typos.toml](.typos.toml).
- **LLVM (macOS only)**: Required to build `utoo-wasm` on macOS (the compiled output is used by `@utoo/web`). The dependency `libmimalloc-sys` compiles C code to `wasm32-unknown-unknown`; Apple Clang does not support that target. Install: `brew install llvm`. Then add the following to your shell config (e.g. `~/.zshrc` or `~/.bash_profile`) so the build uses Homebrew’s clang:
  ```bash
export PATH="$(brew --prefix llvm)/bin:$PATH"
export CC="$(brew --prefix llvm)/bin/clang"
export AR="$(brew --prefix llvm)/bin/llvm-ar"
  ```
  Restart the terminal or run `source ~/.zshrc` (or your config file) for the changes to take effect. 

## 📦 Submodules

Utoo leverages a fork of Next.js to access and modify Turbopack crates.

1.  **Initialize Submodules**:
    If you haven't cloned with `--recursive`, run:
    ```bash
    git submodule update --init --recursive
    ```

2.  **Next.js Integration**:
    The `next.js` folder is a git submodule pointing to `utooland/next.js`. Many Rust crates in `crates/` depend on Turbopack crates located within this submodule (e.g., `next.js/turbopack/crates/turbo-tasks`).

    > [!IMPORTANT]
    > When updating dependencies in `Cargo.toml` that point to the submodule, ensure the paths remain correct and consistent with the submodule's structure.

## 🚀 Setup

1. **Clone the repository**:
   ```bash
   git clone https://github.com/utooland/utoo.git
   cd utoo
   ```

2. **Install dependencies**:
   ```bash
   npm install
   ```

## 💻 Development Workflow

We use [Turborepo](https://turbo.build/) to manage tasks across the monorepo.

### Building

To build all packages and crates:

```bash
npm run build
# or
npx turbo run build
```

To build a specific package (e.g., `@utoo/pack`):

```bash
npx turbo run build --filter=@utoo/pack
```

### Development

To start the development server (primarily for `@utoo/web` and examples):

```bash
npm run dev
```

### Testing

We have both Rust and JavaScript tests.

```bash
# Run all tests
npm run test

# Run Rust tests only
cargo test

# Run PM tests
cargo test -p utoo-pm

# Run Pack tests
cargo test -p pack-tests

# Update Pack tests snapshots
UPDATE=1 cargo test -p pack-tests

# Run PM E2E tests
./e2e/utoo-pm.sh
```

## 📮 Submitting Changes

1.  **Create a Branch**: Use a descriptive name like `feat/awesome-feature` or `fix/bug-id`.
2.  **Commit Messages**: Follow [Conventional Commits](https://www.conventionalcommits.org/). Use scopes like `pm`, `pack`, or `wasm` (e.g., `feat(pack): add new loader`).
3.  **Lint & Format**: Run `npm run biome` before committing.
4.  **Open a PR**: Provide a clear description of your changes and link any related issues.

## 📂 Project Structure

- **`crates/`**: Rust source code for the core logic and native extensions.
  - `pack-core`: Core bundler logic based on Turbopack.
  - `pm`: The `utoo` package manager implementation.
  - `pack-napi`: Node.js bindings via NAPI-RS.
  - `utoo-wasm`: WebAssembly bindings for web usage.
- **`packages/`**: Node.js packages.
  - `@utoo/pack`: The main bundler package, including the **Webpack compatibility layer**.
  - `@utoo/pack-cli`: Command-line interface for the bundler.
  - `@utoo/pack-shared`: Shared utilities and types.
  - `@utoo/web`: Web-compatible version of the toolchain.
- **`next.js/`**: Git submodule containing the Turbopack source code used by the core.
- **`examples/`**: Example projects to test and demonstrate features.

## 🚢 Release

Releases are managed via GitHub Actions.
- **`pack-release.yml`**: Releases `@utoo/pack`, `@utoo/pack-cli`, and `@utoo/pack-shared`.
- **`utooweb-release.yml`**: Releases `@utoo/web`.

## 📄 License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
