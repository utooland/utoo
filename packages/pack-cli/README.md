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

### Production Build

Build your project for production:
```bash
utx up build
```

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

