> ⚠️ Notice: we are working on making a better bundler on top of [Turbopack](https://nextjs.org/docs/app/api-reference/turbopack), see <https://github.com/utooland/utoo/issues/1872>.
>
> If you encountered some critical problems when using current Mako, you can report issues at [Mako 0.x Feedback in discussions](https://github.com/utooland/utoo/discussions/categories/mako-0-x-feedback).

----

<div align="center">
<img src="https://mdn.alipayobjects.com/huamei_botco4/afts/img/357RTIva8S8AAAAAAAAAAAAADnNMAQFr/original" alt="Utoo Logo" width="80"/>
<h1>🌖 Utoo</h1>
<p><strong>Unified Toolchain: Open & Optimized</strong></p>
</div>

---

Utoo is a modern, high-performance frontend toolchain designed to provide a unified and optimized experience for frontend development. It combines a fast package manager, a powerful bundler, and a flexible command mounting system into a single, cohesive ecosystem.

## 📦 Core Components

- **`utoo`**: A high-performance package manager written in Rust, focusing on speed and reliability for dependency resolution and installation.
- **`@utoo/pack`**: A next-generation bundler built on top of [Turbopack](https://turbo.build/pack), providing extreme build speeds and modern features.
- **`ut`**: A powerful command mounting system that allows you to unify your development workflow by aliasing and configuring global or local commands.

## 🚀 Quick Start

### Installation

Install the core toolchain globally:

```bash
npm install -g utoo
```

If you need build capabilities, install the bundler:

```bash
npm install -g @utoo/pack
```

### Basic Usage

#### Package Management

```bash
# Install dependencies
utoo install

# Run a script from package.json
utoo run dev

# Execute a command from a package
utoo x create-react-app my-app
```

#### Bundling

```bash
# Build your project
utoo-pack build

# Start development mode
utoo-pack dev
```

#### Command Mounting (`ut`)

The `ut` command acts as a proxy to your configured tools:

```bash
# Configure a global alias
ut config set install.cmd "utoo install" --global

# Now you can just run
ut install
```

## ✨ Key Features

- ⚡ **Extreme Performance**: Core logic implemented in Rust for maximum speed.
- 🔗 **Unified Workflow**: One toolchain to manage dependencies, build projects, and run commands.
- 🛠️ **Turbopack Powered**: Leverages the power of Turbopack for incremental builds and fast HMR.
- 🔧 **Highly Configurable**: Flexible configuration system for both the package manager and the bundler.
- 📦 **Monorepo Ready**: Built-in support for monorepo structures and workspace management.

## 📂 Project Structure

- [crates/](crates/): Rust-based core logic.
  - [pack-core/](crates/pack-core/): Core bundler logic based on Turbopack.
  - [pm/](crates/pm/): The `utoo` package manager implementation.
  - [pack-napi/](crates/pack-napi/): Node.js bindings for the bundler.
- [packages/](packages/): Node.js packages and CLI.
  - [pack/](packages/pack/): The main `@utoo/pack` package.
  - [pack-cli/](packages/pack-cli/): CLI for the bundler.
  - [utoo-web/](packages/utoo-web/): Web-compatible version of the toolchain.
- [examples/](examples/): Various example projects demonstrating Utoo's capabilities.

## 🤝 Contributing

We welcome contributions! Please see our [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to get started.

## 📄 License

Utoo is licensed under the [MIT License](LICENSE).
[values]
"install.cmd" = "utoo install"
"build.cmd" = "utoo-pack build"
"*.cmd" = "utoo"
```

### 📋 Available Commands

```bash
# Show help and available commands
ut --help

# List all configured commands
ut config list

# Get specific command configuration
ut config get install.cmd

# Set command configuration
ut config set install.cmd "utoo install" --global
```

## 🔌 Command Mounting

Utoo provides a powerful command mounting system that allows you to extend the toolchain with custom commands. This is particularly useful for project-specific scripts and workflows.

### 📜 Package.json Scripts

Any script defined in your `package.json` can be executed directly through `ut`:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "test": "jest"
  }
}
```

You can run these scripts using:

```bash
# Using the run command
ut run dev
# or shorthand
ut r dev

# Direct command execution
ut dev
ut build
ut test
```

### 🛠️ Custom Commands

You can create custom commands by adding them to your project's configuration. These commands can be:

1. **Shell Scripts**: Simple shell commands or scripts
2. **Node.js Scripts**: JavaScript/TypeScript files
3. **Binary Executables**: Any executable in your project

### 🌍 Command Execution Environment

When executing commands, Utoo provides:

- Access to all project dependencies
- Environment variables from your project
- Proper working directory context
- Node.js binary path resolution

### ⚡ Command Hooks

Utoo supports various command hooks that can be used to extend command behavior:

- `preinstall`: Run before package installation
- `install`: Run during package installation
- `postinstall`: Run after package installation
- `prepare`: Run before package preparation
- `preprepare`: Run before package preparation
- `postprepare`: Run after package preparation

## 📋 Commands

### 📦 Package Management Commands

#### 📥 Install Dependencies

```bash
# Install project dependencies
ut install
# or shorthand
ut i

# Install specific package
ut install <package-name>
# Example: install lodash
ut install lodash

# Install as dev dependency
ut install <package-name> --save-dev

# Install as peer dependency
ut install <package-name> --save-peer

# Install as optional dependency
ut install <package-name> --save-optional

# Global installation
ut install <package-name> -g
```

#### 🗑️ Uninstall Dependencies

```bash
# Uninstall specific package
ut uninstall <package-name>
# or shorthand
ut un <package-name>
```

#### 🔄 Update Dependencies

```bash
# Update all dependencies
ut update
# or shorthand
ut u
```

### 🏗️ Build Commands

#### 🔨 Rebuild Dependencies

```bash
# Rebuild all dependencies
ut rebuild
# or shorthand
ut rb
```

#### 🧹 Clean Cache

```bash
# Clean all cache
ut clean

# Clean specific package cache
ut clean <package-pattern>
# Example: clean all react related packages
ut clean "react*"
```

#### 📊 Dependency Analysis

```bash
# Analyze project dependencies
ut deps
# or shorthand
ut d

# Only analyze workspace dependencies
ut deps --workspace-only
```

### ⚙️ Common Options

All commands support the following common options:

- `--verbose`: Show detailed output
- `--registry <url>`: Set npm registry URL
- `--legacy-peer-deps`: Use legacy peer dependency handling
- `--ignore-scripts`: Skip running dependency scripts

## 🔨 Build from Source

```bash
# Build project
cargo build --release

# Add binary to PATH
export PATH=$PATH:$(pwd)/target/release
```

### 📦 Install Dependencies

```bash
# Install project dependencies
ut
```
### 🛠️ Bundler

Utoo includes a high-performance bundler that supports various build scenarios:

#### 🚀 Basic Usage

Install `@utoo/pack-cli`:

```bash
ut install @utoo/pack-cli --save-dev
```

Then you can bundle your application or library with utoopack:

```json
{
  "scripts": {
    "build": "utoo-pack build",
    "dev": "utoo-pack dev",
    "analyze": "ANALYZE=true utoo-pack build"
  }
}
```

### Bundler features
You can track features supported of bundler at [packages/pack/docs/features-list.md](https://github.com/utooland/utoo/blob/next/packages/pack/docs/features-list.md)

Currently, utoopack's devServer does not support generating an HTML file for previews. You'll need to create one manually to view the output assets. We plan to add support for this in the future.

Or you can use utoopack with a framework like [umi](https://github.com/umijs/umi). Note that your umi version must be `v4.5.0` or higher (`v4.6.0` or newer is recommended).

It's easier to enable:

```ts
// .umirc.ts
import { defineConfig } from '@umijs/max';

export default defineConfig({
  utoopack: {}
})
```

#### 📚 Example Projects

We provide several example projects to demonstrate different usage scenarios:

- `examples/with-antd`: Ant Design component library integration
- `examples/with-sass`: Sass style processing
- `examples/with-less`: Less style processing
- `examples/with-style-loader`: Style loader usage
- `examples/with-library`: Library mode build
- `more to come ...`

### 🏃 Run Bundler

```bash
# Build local development environment
git submodule update --init
cd packages/pack
ut build:local

# Build by native
cargo run --bin pack-cli -- --mode build  --project-dir examples/with-antd --root-dir .
cargo run --bin pack-cli -- --mode dev --watch true --project-dir examples/with-antd --root-dir .

# Build the napi package
cd packages/pack
npm run build:local

cd ../../examples/with-antd
npm run build
npm run dev
```

## 📁 Project Structure

```
.
├── crates/          # Rust core libraries
│   ├── cli/         # Command line tools
│   ├── core/        # Core functionality
│   ├── pack-*       # Bundler related modules
│   ├── utoo-web     # Unified ut package manager and @utoo/pack into browser WebAssembly
├── packages/        # Package management code
├── examples/        # Example projects
└── vendor/          # Third-party dependencies
```
