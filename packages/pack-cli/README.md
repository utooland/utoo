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
utoopack dev
```

### Production Build

Build your project for production:

```bash
utoopack build
```

### Webpack Compatibility Mode

If you have an existing `webpack.config.js`, you can run `@utoo/pack` in compatibility mode:

```bash
utoopack build --webpack
# or
utoopack dev --webpack
```

## 🛠️ Commands

- `utoopack dev`: Start development server.
- `utoopack build`: Build for production.
- `utoopack help`: Show help for all commands.

## 📄 License

[MIT](./LICENSE)

