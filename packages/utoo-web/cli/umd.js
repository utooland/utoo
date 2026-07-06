const path = require('path');
const fs = require('fs/promises');
const webpack = require('webpack');
const yargs = require('yargs');
const MinimizerPlugin = require('minimizer-webpack-plugin');
const stdLibBrowser = require('node-stdlib-browser');
const { NodeProtocolUrlPlugin } = require('node-stdlib-browser/helpers/webpack/plugin');

const argv = yargs(process.argv.slice(2))
  .option('entry', {
    alias: 'e',
    description: 'The entry file path',
    type: 'string',
    demandOption: true,
  })
  .option('output', {
    alias: 'o',
    description: 'The output file path',
    type: 'string',
    demandOption: true,
  })
  .option('base64-url', {
    alias: 'b64',
    description: 'Export bundled content as a data:application/javascript;base64 URL string (ESM)',
    type: 'boolean',
  })
  .option('target', {
    alias: 't',
    description: 'Webpack target',
    type: 'string',
  })
  .option('no-polyfill', {
    alias: 'np',
    description: 'Disable custom polyfills (fs, fs/promises) and use node-stdlib-browser stubs instead',
    type: 'boolean',
    default: false,
  })
  .help()
  .alias('help', 'h').argv;

const { entry, output, target, base64Url, noPolyfill } = argv;

const outputFullPath = path.resolve(process.cwd(), output);
// If base64-url, use a temp dir. Otherwise output directly.
const outputDir = base64Url
  ? path.resolve(process.cwd(), './tmp') 
  : path.dirname(outputFullPath);
const outputFilename = path.basename(outputFullPath);

const polyfillAliases = noPolyfill ? {} : {
  fs: path.resolve(__dirname, '../src/webpackLoaders/polyfills/fsPolyfill.ts'),
  'fs/promises': path.resolve(__dirname, '../src/webpackLoaders/polyfills/fsPromisesPolyfill.ts'),
};

const config = {
  mode: 'production',
  entry,
  output: {
    path: outputDir,
    filename: outputFilename,
    library: {
      type: 'umd',
      export: 'default',
    },
    globalObject: 'self',
    // Do not use clean: true here as it might wipe shared output directories
  },
  resolve: {
    extensions: ['.ts', '.js'],
    alias: {
      ...stdLibBrowser,
      ...polyfillAliases,
      v8: path.resolve(__dirname, './mocks/v8.js'),
      perf_hooks: path.resolve(__dirname, './mocks/perf_hooks.js'),
      env: path.resolve(__dirname, './mocks/env.js'),
    },
  },
  module: {
    rules: [
      {
        test: /\.m?js/,
        resolve: {
          fullySpecified: false,
        },
      },
      {
        test: /\.ts$/,
        exclude: /node_modules/,
        use: {
          loader: 'ts-loader',
          options: {
            transpileOnly: true,
          },
        },
      },
      {
        test: /utoo\/index_bg\.wasm$/,
        type: 'asset/resource',
        generator: {
          emit: false,
        },
      },
    ],
  },
  plugins: [
    new NodeProtocolUrlPlugin(),
    new webpack.ProvidePlugin({
      process: stdLibBrowser.process,
      Buffer: stdLibBrowser.buffer,
    }),
  ],
  optimization: {
    moduleIds: 'named',
    minimizer: [
      (compiler) => {
        new MinimizerPlugin({
          terserOptions: {
            mangle: false,
            keep_fnames: true,
            keep_classnames: true,
          },
        }).apply(compiler);
      },
    ],
  },
  devtool: false,
  target,
};

console.log(`✨ Starting build for ${output}...`);

webpack(config, async (err, stats) => {
  if (err) {
    console.error('❌ Webpack configuration error:', err);
    process.exit(1);
  }

  if (stats.hasErrors()) {
    console.error('❌ Build failed with errors:');
    console.error(stats.toString({ colors: true, modules: false }));
    process.exit(1);
  }

  if (base64Url) {
    try {
      const bundlePath = path.join(outputDir, outputFilename);
      const content = await fs.readFile(bundlePath);
      const base64 = content.toString('base64');
      const dataUri = `data:application/javascript;base64,${base64}`;
      const finalContent = `export default ${JSON.stringify(dataUri)};\n`;

      await fs.writeFile(outputFullPath, finalContent, 'utf-8');
      
      const dtsPath = outputFullPath.replace(/\.js$/, '.d.ts');
      await fs.writeFile(dtsPath, 'declare const url: string;\nexport default url;\n', 'utf-8');

      await fs.unlink(bundlePath).catch(() => {});
      console.log(`✅ Build and base64-url successful: ${output}`);
    } catch (e) {
      console.error('❌ Failed to write base64-url output:', e);
      process.exit(1);
    }
  } else {
    console.log(`✅ Build successful: ${output}`);
  }
});
