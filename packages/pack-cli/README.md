# @utoo/pack-cli

> 🌖 Command-line interface for `@utoo/pack`, the high-performance bundler powered by [Turbopack](https://turbo.build/pack).

## 📦 Installation

```bash
ut install @utoo/pack-cli --save-dev
```

## 🚀 Usage

### Config setup

Add a `utoopack.json` config file in the root directory:

```json
{
  "$schema": "@utoo/pack/config_schema.json",
  "entry": [
    {
      "import": "./src/index.ts",
      "html": {
        "template": "./index.html"
      }
    }
  ],
  "output": {
    "path": "./dist",
    "filename": "[name].[contenthash:8].js",
    "chunkFilename": "[name].[contenthash:8].js",
    "clean": true
  },
  "sourceMaps": true
}
```

### Development Mode

Start the development server with Hot Module Replacement (HMR):

```bash
utx up dev
```

SPA history fallback is enabled by default: navigation to routes such as `/home`
or `/home/settings` serves the output directory's `index.html`, so React Router
does not need a self-proxy. Existing files and configured API proxies take
precedence. Missing files with a dot in the final path segment and requests that
do not accept HTML retain their normal error responses.

Use `utx up dev --no-history-api-fallback` to disable this behavior. The default
applies only to the CLI: calling `@utoo/pack.serve()` directly keeps fallback
disabled, preserving framework integrations' existing routing. API callers can
opt in with `historyApiFallback: true` in the fourth argument (server options).

### Production Build

Build your project for production:
```bash
utx up build
```

### Worktree and Symlink Debugging

To reuse `node_modules` from another checkout or debug a linked dependency,
set the root to a common parent containing both the project and the real
dependency paths:

```bash
UTOO_ROOT_PATH=/path/to/common-parent utx up dev
UTOO_ROOT_PATH=.. utx up build
```

`UTOO_ROOT_PATH` overrides `--root`, config `rootPath`, and automatic root
detection. Relative environment paths resolve from the current working
directory; an empty value is ignored. The project directory stays unchanged.

### Webpack Compatibility Mode

If you have an existing `webpack.config.js`, you can run `@utoo/pack` in compatibility mode:

```bash
utx up build --webpack
# or
utx up dev --webpack
```

## 🛠️ Commands

- `utx up dev`: Start development server.
- `utx up build`: Build for production.
- `utx up help`: Show help for all commands.

You can see [examples/*](../../examples/) for more usage scenarios.

## 📄 License

[MIT](./LICENSE)

