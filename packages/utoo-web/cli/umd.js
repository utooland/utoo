const path = require('path');
const fs = require('fs/promises');
const webpack = require('webpack');
const yargs = require('yargs');
const TerserPlugin = require('terser-webpack-plugin');
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
  .option('stringify-esm', {
    alias: 'se',
    description: 'Export content as ESM string',
    type: 'boolean',
  })
  .option('stringify-cjs', {
    alias: 'sc',
    description: 'Export content as CJS string',
    type: 'boolean',
  })
  .option('target', {
    alias: 't',
    description: 'Webpack target',
    type: 'string',
  })
  .help()
  .alias('help', 'h').argv;

const { entry, output, target, stringifyEsm, stringifyCjs } = argv;
const isStringify = stringifyEsm || stringifyCjs;

const outputFullPath = path.resolve(process.cwd(), output);
// If stringifying, use a temp dir. Otherwise output directly.
const outputDir = isStringify 
  ? path.resolve(process.cwd(), './tmp') 
  : path.dirname(outputFullPath);
const outputFilename = path.basename(outputFullPath);

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
      // Use the real polyfill instead of the stub
      fs: path.resolve(__dirname, '../src/webpackLoaders/polyfills/fsPolyfill.ts'),
      'fs/promises': path.resolve(__dirname, '../src/webpackLoaders/polyfills/fsPromisesPolyfill.ts'),
      v8: path.resolve(__dirname, './mocks/v8.js'),
      perf_hooks: path.resolve(__dirname, './mocks/perf_hooks.js'),
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
        new TerserPlugin({
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

  if (isStringify) {
    try {
      const bundlePath = path.join(outputDir, outputFilename);
      const content = await fs.readFile(bundlePath, 'utf-8');
      const escapedContent = JSON.stringify(content);
      
      const finalContent = stringifyEsm
        ? `export default ${escapedContent};\n`
        : `module.exports = ${escapedContent};\n`;

      await fs.writeFile(outputFullPath, finalContent, 'utf-8');
      // Optional: clean up temp file
      // await fs.unlink(bundlePath); 
      console.log(`✅ Build and stringify successful: ${output}`);
    } catch (e) {
      console.error('❌ Failed to write stringified output:', e);
      process.exit(1);
    }
  } else {
    console.log(`✅ Build successful: ${output}`);
  }
});
