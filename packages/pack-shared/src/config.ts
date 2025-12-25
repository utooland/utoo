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

export interface ExperimentalConfig {}

export type TurbopackRuleConfigItem =
  | TurbopackRuleConfigItemOptions
  | { [condition: string]: TurbopackRuleConfigItem }
  | false;

/**
 * @deprecated Use `TurbopackRuleConfigItem` instead.
 */
export type TurbopackLoaderItem =
  | string
  | {
      loader: string;
      // At the moment, Turbopack options must be JSON-serializable, so restrict values.
      options: Record<string, JSONValue>;
    };

export type TurbopackRuleConfigItemOrShortcut =
  | TurbopackLoaderItem[]
  | TurbopackRuleConfigItem;

export type TurbopackRuleConfigItemOptions = {
  loaders: TurbopackLoaderItem[];
  as?: string;
};

export type TurbopackRuleCondition = {
  path: string | RegExp;
};

export interface ModuleOptions {
  rules?: Record<string, TurbopackRuleConfigItem>;
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
    clean?: boolean;
    copy?: Array<
      | {
          from: string;
          to?: string;
        }
      | string
    >;
    publicPath?: string;
  };
  target?: string;
  sourceMaps?: boolean;
  define?: Record<string, string>;
  provider?: ProviderConfig;
  optimization?: {
    moduleIds?: "named" | "deterministic";
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
    emotion?: boolean | EmotionConfig;
  };
  images?: {
    inlineLimit?: number;
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

export interface EmotionConfig {
  sourceMap?: boolean;
  autoLabel?: "dev-only" | "always" | "never";
  labelFormat?: string;
  importMap?: {
    [importName: string]: {
      [exportName: string]: {
        canonicalImport?: [string, string];
        styledBaseImport?: [string, string];
      };
    };
  };
}

export type JSONValue =
  | string
  | number
  | boolean
  | JSONValue[]
  | { [k: string]: JSONValue };

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
