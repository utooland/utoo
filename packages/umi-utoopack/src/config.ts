import type { IOpts as IConfigOpts } from "@umijs/bundler-webpack";
import { getConfig } from "@umijs/bundler-webpack";
import { lodash } from "@umijs/utils";
import type { BundleOptions, WebpackConfig } from "@utoo/pack";
import { compatOptionsFromWebpack } from "@utoo/pack";
import type { IOpts } from "./types";

export async function getProdUtooPackConfig(
  opts: IOpts,
): Promise<BundleOptions> {
  console.log("entry: ", opts.entry);
  const webpackConfig = await getConfig({
    cwd: opts.cwd,
    rootDir: opts.rootDir,
    env: "production" as any,
    entry: opts.entry,
    userConfig: opts.config,
    analyze: process.env.ANALYZE,
    babelPreset: opts.babelPreset,
    extraBabelPlugins: [
      ...(opts.beforeBabelPlugins || []),
      ...(opts.extraBabelPlugins || []),
    ],
    extraBabelPresets: [
      ...(opts.beforeBabelPresets || []),
      ...(opts.extraBabelPresets || []),
    ],
    extraBabelIncludes: opts.config.extraBabelIncludes,
    chainWebpack: opts.chainWebpack,
    modifyWebpackConfig: opts.modifyWebpackConfig,
    pkg: opts.pkg,
    disableCopy: opts.disableCopy,
  });

  let utooBundlerOpts = compatOptionsFromWebpack({
    ...lodash.omit(webpackConfig, ["target", "module"]),
    compatMode: true,
  } as WebpackConfig);

  utooBundlerOpts = {
    ...utooBundlerOpts,
    config: {
      ...utooBundlerOpts.config,
      styles: {
        less: {
          modifyVars: opts.config.theme,
          javascriptEnabled: true,
          ...opts.config.lessLoader,
        },
        sass: opts.config.sassLoader ?? undefined,
      },
    },
  } as BundleOptions;

  return utooBundlerOpts;
}

export type IDevOpts = {
  afterMiddlewares?: any[];
  beforeMiddlewares?: any[];
  onDevCompileDone?: Function;
  onProgress?: Function;
  onMFSUProgress?: Function;
  port?: number;
  host?: string;
  ip?: string;
  babelPreset?: any;
  chainWebpack?: Function;
  modifyWebpackConfig?: Function;
  beforeBabelPlugins?: any[];
  beforeBabelPresets?: any[];
  extraBabelPlugins?: any[];
  extraBabelPresets?: any[];
  cwd: string;
  rootDir?: string;
  config: Record<string, any>;
  entry: Record<string, string>;
  mfsuStrategy?: "eager" | "normal";
  mfsuInclude?: string[];
  srcCodeCache?: any;
  startBuildWorker?: (deps: any[]) => Worker;
  onBeforeMiddleware?: Function;
  disableCopy?: boolean;
} & Pick<IConfigOpts, "cache" | "pkg">;

export async function getDevUtooPackConfig(
  opts: IDevOpts,
): Promise<BundleOptions> {
  let webpackConfig = await getConfig({
    cwd: opts.cwd,
    rootDir: opts.rootDir,
    env: "development" as any,
    entry: opts.entry,
    userConfig: opts.config,
    babelPreset: opts.babelPreset,
    extraBabelPlugins: [
      ...(opts.beforeBabelPlugins || []),
      ...(opts.extraBabelPlugins || []),
    ],
    extraBabelPresets: [
      ...(opts.beforeBabelPresets || []),
      ...(opts.extraBabelPresets || []),
    ],
    extraBabelIncludes: opts.config.extraBabelIncludes,
    chainWebpack: opts.chainWebpack,
    modifyWebpackConfig: opts.modifyWebpackConfig,
    // TO avoild bundler webpack add extra entry.
    hmr: false,
    analyze: process.env.ANALYZE,
  });
  // console.log('webpackConfig: ', JSON.stringify(webpackConfig.entry, null, 2))

  let utooBundlerOpts = compatOptionsFromWebpack({
    ...lodash.omit(webpackConfig, ["target", "module"]),
    compatMode: true,
  } as WebpackConfig);

  utooBundlerOpts = {
    ...utooBundlerOpts,
    config: {
      ...utooBundlerOpts.config,
      styles: {
        less: {
          modifyVars: opts.config.theme,
          javascriptEnabled: true,
          ...opts.config.lessLoader,
        },
        sass: opts.config.sassLoader ?? undefined,
      },
    },
    watch: {
      enable: true,
    },
    dev: true,
  };

  return utooBundlerOpts;
}
