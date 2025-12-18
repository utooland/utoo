# Contributing to Utoo

Thank you for your interest in contributing to Utoo! This document provides a guide for setting up your development environment and working on the project.

## 🛠️ Prerequisites

Before you begin, ensure you have the following installed:

- **Node.js**: Version 20 or higher.
- **npm**: Version 10 or higher (managed via `packageManager` in `package.json`).
- **Rust**: We use a specific nightly version defined in [rust-toolchain.toml](rust-toolchain.toml). It will be automatically installed when you run Cargo commands.
- **wasm-bindgen-cli**: Required for building the web version.
  ```bash
  cargo install wasm-bindgen-cli@0.2.104
  ```

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

# Run E2E tests
./e2e/utoo-pm.sh
```

### Linting & Formatting

We use [Biome](https://biomejs.dev/) for linting and formatting.

```bash
# Check and fix linting/formatting issues
npm run biome
```

## 📂 Project Structure

- **`crates/`**: Rust source code for the core logic and native extensions.
  - `pack-core`: Core bundler logic based on Turbopack.
  - `pm`: The `utoo` package manager implementation.
  - `pack-napi`: Node.js bindings via NAPI-RS.
  - `utoo-wasm`: WebAssembly bindings for web usage.
- **`packages/`**: Node.js packages.
  - `@utoo/pack`: The main bundler package.
  - `@utoo/pack-cli`: Command-line interface for the bundler.
  - `@utoo/pack-shared`: Shared utilities and types.
  - `@utoo/web`: Web-compatible version of the toolchain.
- **`examples/`**: Example projects to test and demonstrate features.

## 🚢 Release

Releases are managed via GitHub Actions.
- **`pack-release.yml`**: Releases `@utoo/pack`, `@utoo/pack-cli`, and `@utoo/pack-shared`.
- **`utooweb-release.yml`**: Releases `@utoo/web`.

## 📄 License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
