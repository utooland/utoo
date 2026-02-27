export interface EntryOptions {
  name?: string;
  import: string;
  library?: LibraryOptions;
  /**
   * Configuration for generating an HTML file for this entry point.
   * When specified, an HTML file will be generated with the entry's assets injected.
   *
   * Similar to https://github.com/jantimon/html-webpack-plugin#options
   */
  html?: HtmlConfig;
}

export interface LibraryOptions {
  name?: string;
  export?: Array<string>;
}

export interface DefineEnv {
  client: RustifiedEnv;
  edge: RustifiedEnv;
  nodejs: RustifiedEnv;
}

export type RustifiedEnv = { name: string; value: string }[];

export interface ExperimentalConfig {
  swcPlugins?: [string, any][];
  dynamicIO?: boolean;
  useCache?: boolean;
  cacheHandlers?: Record<string, string>;
  esmExternals?: boolean | "loose";
  ppr?: boolean | "incremental";
  taint?: boolean;
  reactCompiler?: boolean | any;
  viewTransition?: boolean;
  serverActions?: boolean | any;
}

export type JSONValue =
  | string
  | number
  | boolean
  | JSONValue[]
  | { [k: string]: JSONValue };

// At the moment, Turbopack options must be JSON-serializable, so restrict values.
export type TurbopackLoaderOptions = Record<string, JSONValue>;

export type TurbopackLoaderItem =
  | string
  | {
      loader: string;
      options?: TurbopackLoaderOptions;
    };

export type TurbopackLoaderBuiltinCondition =
  | "browser"
  | "foreign"
  | "development"
  | "production"
  | "node"
  | "edge-light";

export type TurbopackRuleCondition =
  | { all: TurbopackRuleCondition[] }
  | { any: TurbopackRuleCondition[] }
  | { not: TurbopackRuleCondition }
  | TurbopackLoaderBuiltinCondition
  | {
      path?: string | RegExp;
      content?: RegExp;
      query?: string | RegExp;
      contentType?: string | RegExp;
    };

export type TurbopackRuleConfigItem = {
  loaders: TurbopackLoaderItem[];
  as?: string;
  condition?: TurbopackRuleCondition;
};

/**
 * This can be an object representing a single configuration, or a list of
 * loaders and/or rule configuration objects.
 *
 * - A list of loader path strings or objects is the "shorthand" syntax.
 * - A list of rule configuration objects can be useful when each configuration
 *   object has different `condition` fields, but still match the same top-level
 *   path glob.
 */
export type TurbopackRuleConfigCollection =
  | TurbopackRuleConfigItem
  | (TurbopackLoaderItem | TurbopackRuleConfigItem)[];

export interface ModuleOptions {
  rules?: Record<string, TurbopackRuleConfigCollection>;
}

export interface ResolveOptions {
  alias?: Record<string, string | string[] | Record<string, string | string[]>>;
  extensions?: string[];
}

export type ExternalType = "script" | "commonjs" | "esm" | "global";

export interface ExternalAdvanced {
  root: string;
  type?: ExternalType;
  script?: string;
}

export type ExternalConfig = string | ExternalAdvanced;

/**
 * Provider configuration for automatic module imports.
 * Similar to webpack's ProvidePlugin.
 *
 * @example
 * ```ts
 * provider: {
 *   // Provides `$` as `import $ from 'jquery'`
 *   $: 'jquery',
 *   // Provides `Buffer` as `import { Buffer } from 'buffer'`
 *   Buffer: ['buffer', 'Buffer'],
 * }
 * ```
 */
export type ProviderConfig = Record<string, string | [string, string]>;

export interface ConfigComplete {
  entry: EntryOptions[];
  mode?: "production" | "development";
  module?: ModuleOptions;
  resolve?: ResolveOptions;
  externals?: Record<string, ExternalConfig>;
  output?: {
    path?: string;
    type?: "standalone" | "export";
    filename?: string;
    chunkFilename?: string;
    cssFilename?: string;
    cssChunkFilename?: string;
    assetModuleFilename?: string;
    clean?: boolean;
    copy?: Array<
      | {
          from: string;
          to?: string;
        }
      | string
    >;
    publicPath?: string;
    chunkLoadingGlobal?: string;
    entryRootExport?: string;
  };
  target?: string;
  sourceMaps?: boolean;
  define?: Record<string, string>;
  provider?: ProviderConfig;
  optimization?: {
    moduleIds?: "named" | "deterministic";
    noMangling?: boolean;
    minify?: boolean;
    treeShaking?: boolean;
    splitChunks?: Record<
      "js" | "css",
      {
        minChunkSize?: number;
        maxChunkCountPerGroup?: number;
        maxMergeChunkSize?: number;
      }
    >;
    modularizeImports?: Record<
      string,
      {
        transform: string | Record<string, string>;
        preventFullImport?: boolean;
        skipDefaultConversion?: boolean;
        handleDefaultImport?: boolean;
        handleNamespaceImport?: boolean;
        style?: string;
      }
    >;
    packageImports?: string[];
    transpilePackages?: string[];
    removeConsole?:
      | boolean
      | {
          exclude?: string[];
        };
    concatenateModules?: boolean;
    removeUnusedExports?: boolean;
    removeUnusedImports?: boolean;
    nestedAsyncChunking?: boolean;
    wasmAsAsset?: boolean;
  };
  styles?: {
    less?: {
      implementation?: string;
      [key: string]: any;
    };
    sass?: {
      implementation?: string;
      [key: string]: any;
    };
    inlineCss?: {
      insert?: string;
      injectType?: string;
    };
    styledJsx?:
      | boolean
      | {
          useLightningcss?: boolean;
        };

    styledComponents?: boolean | StyledComponentsConfig;
  };
  images?: {
    inlineLimit?: number;
  };
  react?: {
    runtime?: "automatic" | "classic";
    importSource?: string;
  };
  stats?: boolean;
  persistentCaching?: boolean;
  nodePolyfill?: boolean;
  devServer?: {
    hot?: boolean;
    port?: number;
    host?: string;
    https?: boolean;
  };
  cacheHandler?: string;
  experimental?: ExperimentalConfig;
}

export interface HtmlConfig {
  template?: string;
  templateContent?: string;
  filename?: string;
  title?: string;
  inject?: boolean | "body" | "head";
  scriptLoading?: "blocking" | "defer" | "module";
  meta?: Record<string, string | { [key: string]: string }>;
  output?: {
    path?: string;
  };
}

export interface StyledComponentsConfig {
  /**
   * Enabled by default in development, disabled in production to reduce file size,
   * setting this will override the default for all environments.
   */
  displayName?: boolean;
  topLevelImportPaths?: string[];
  ssr?: boolean;
  fileName?: boolean;
  meaninglessFileNames?: string[];
  minify?: boolean;
  transpileTemplateLiterals?: boolean;
  namespace?: string;
  pure?: boolean;
  cssProp?: boolean;
}

export interface BundleOptions {
  /**
   * The utoo pack configs.
   */
  config: ConfigComplete;

  /**
   * A map of environment variables to use when compiling code.
   */
  processEnv?: Record<string, string>;

  defineEnv?: DefineEnv;

  /**
   * Whether to watch the filesystem for file changes.
   */
  watch?: {
    enable: boolean;
    pollIntervalMs?: number;
  };

  /**
   * The mode of utoopack.
   */
  dev?: boolean;

  /**
   * The build id.
   */
  buildId?: string;

  /**
   * Absolute path for `@utoo/pack`.
   */
  packPath?: string;
}
