const path = require('path');
const fs = require('fs/promises');
const webpack = require('webpack');
const yargs = require('yargs');
const TerserPlugin = require('terser-webpack-plugin');
const stdLibBrowser = require('node-stdlib-browser');
const { NodeProtocolUrlPlugin } = require('node-stdlib-browser/helpers/webpack/plugin');

const TMP_DIR = './tmp';

function createUmdConfig(entry, umdFilename, target) {
  return {
    mode: 'production',
    entry: entry,
    output: {
      path: path.resolve(process.cwd(), TMP_DIR),
      filename: umdFilename,
      library: {
        type: 'umd',
        export: 'default',
      },
      clean: true,
      globalObject: 'self',
    },
    resolve: {
      extensions: ['.ts', '.js'],
      alias: {... stdLibBrowser,
        //  fs: path.join(__dirname, "./fs.js")
        },
    },
    module: {
      rules: [
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
      new webpack.ProvidePlugin({
        process: stdLibBrowser.process,
        Buffer: stdLibBrowser.buffer,
      }),
    ],
    optimization: {
      moduleIds: 'named',
      minimize: false,
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
}

async function writeEsmStringFile(umdFullPath, esmFullPath) {
  const umdContent = await fs.readFile(umdFullPath, 'utf-8');
  const escapedContent = JSON.stringify(umdContent);

  const esmContent = `
export default ${escapedContent};
`;
  console.log('       ✅ Generate stringified esm module successful.');
  await fs.writeFile(esmFullPath, esmContent, 'utf-8');
}

async function writeCjsStringFile(umdFullPath, esmFullPath) {
  const umdContent = await fs.readFile(umdFullPath, 'utf-8');
  const escapedContent = JSON.stringify(umdContent);

  const esmContent = `
module.exports =  ${escapedContent};
`;
  console.log('       ✅ Generate stringified cjs module successful.');
  await fs.writeFile(esmFullPath, esmContent, 'utf-8');
}

const argv = yargs(process.argv.slice(2))
  .option('entry', {
    alias: 'e',
    description: 'The entry file path (e.g., ./src/index.ts)',
    type: 'string',
    demandOption: true,
  })
  .option('output', {
    alias: 'o',
    description: 'The output file path (e.g., ./src/webpackLoaders/workerContent.js)',
    type: 'string',
    demandOption: true,
  })
  .option('stringify-esm', {
    alias: 'se',
    description: 'Whether to convert to esm and export content as string',
    type: 'boolean',
    demandOption: false,
  })
  .option('stringify-cjs', {
    alias: 'sc',
    description: 'Whether to convert to esm and export content as string',
    type: 'boolean',
    demandOption: false,
  })
  .option('target', {
    alias: 't',
    description: 'The output target, meaning webpack target (e.g., node or webworker)',
    type: 'string',
    demandOption: true,
  })
  .help()
  .alias('help', 'h').argv;

const { entry, output, target, stringifyEsm, stringifyCjs } = argv;

const filename = path.basename(output);
const umdFullPath = path.relative(process.cwd(), path.join(TMP_DIR, filename));
const outputFullPath = path.resolve(process.cwd(), output);

console.log(`✨ Starting two-stage build...`);
console.log(`   Entry: ${entry}`);
console.log(`   Umd Output Directory: ${TMP_DIR}`);
console.log(`   Stage 1 (UMD Bundle): ${TMP_DIR}/${filename}`);
console.log(`   Stage 2 (Output String): ${output}`);

const umdConfig = createUmdConfig(entry, filename, target);

const compiler = webpack(umdConfig);

compiler.run(async (err, stats) => {
  if (err) {
    console.error('❌ Webpack build failed with a critical error:', err.stack || err);
    return process.exit(1);
  }

  if (stats.hasErrors()) {
    console.error('❌ Webpack build failed with compilation errors.');
    stats.toJson({ errors: true }).errors.forEach((e) => console.error(e.message));
    return process.exit(1);
  }

  try {
    if (!!stringifyEsm) {
      await writeEsmStringFile(umdFullPath, outputFullPath);
    } else if (!!stringifyCjs) {
      await writeCjsStringFile(umdFullPath, outputFullPath);
    } else {
      const umdContent = await fs.readFile(umdFullPath, 'utf8');
      await fs.writeFile(outputFullPath, umdContent, 'utf-8');
    }
    console.log(`✅ Build successful.`);
  } catch (fsError) {
    console.error('❌ Write output file failed', fsError);
    process.exit(1);
  }
});
