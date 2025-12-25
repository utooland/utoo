# @utoo/pack-cli

> 🌖 Command-line interface for `@utoo/pack`, the high-performance bundler powered by [Turbopack](https://turbo.build/pack).

## 📦 Installation

```bash
npm install -g @utoo/pack-cli
```

## 🚀 Usage

### Development Mode

Start the development server with Hot Module Replacement (HMR):

```bash
up dev
```

### Production Build

Build your project for production:

```bash
up build
```

### Webpack Compatibility Mode

If you have an existing `webpack.config.js`, you can run `@utoo/pack` in compatibility mode:

```bash
up build --webpack
# or
up dev --webpack
```

## 🛠️ Commands

- `up dev`: Start development server.
- `up build`: Build for production.
- `up help`: Show help for all commands.

## 📄 License

[MIT](./LICENSE)

