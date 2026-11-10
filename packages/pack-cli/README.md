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

### Native Bundle Analyzer

```bash
utx up analyze                              # Analyze and serve on localhost:4000
utx up analyze --port 4100
utx up analyze --output                     # Export .utoo/analyze without serving
utx up analyze --output --output-dir reports/bundle
```

Supports browser applications and MPA. The report provides entry selection,
module sizes, a treemap, search and import chains. It uses production settings,
writes to a separate report directory, and leaves existing bundles and
`stats.json` intact. Existing `ANALYZE=true up build` continues to use the
Webpack-compatible analyzer. `--project`, `--root`, `--webpack` and
`--no-tracing` work as for `up build`.

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
- `utx up analyze`: Analyze browser bundles and show a static report.
- `utx up help`: Show help for all commands.

You can see [examples/*](../../examples/) for more usage scenarios.

## 📄 License

[MIT](./LICENSE)

