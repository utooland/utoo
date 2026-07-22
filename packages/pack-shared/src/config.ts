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

/** A named server entry compiled as an independently addressable Node.js bundle. */
export interface ServerEntryOptions {
  name: string;
  import: string;
}

/**
 * The primary server entry. A string preserves the legacy `index` entry name.
 * Use the object form when consumers need a stable entrypoint name in stats.
 */
export type ServerEntry = string | ServerEntryOptions;

export interface LibraryOptions {
  name?: string;
  export?: Array<string>;
}

export type RustifiedEnv = { name: string; value: string }[];

export type JSONValue =
  | string
  | number
  | boolean
  | JSONValue[]
  | { [k: string]: JSONValue };

export interface MdxTransformOptions {
  development?: boolean;
  jsx?: boolean;
  jsxRuntime?: string;
  jsxImportSource?: string;
  providerImportSource?: string;
  mdxType?: "commonmark" | "gfm";
}

export type MdxOptions = boolean | MdxTransformOptions;

export interface ReactCompilerOptions {
  compilationMode?: "infer" | "annotation" | "all";
  target?: "18" | "19";
}

export type ReactCompilerConfig = boolean | ReactCompilerOptions;

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

export type TurbopackModuleType =
  | "asset"
  | "ecmascript"
  | "typescript"
  | "css"
  | "css-module"
  | "json"
  | "wasm"
  | "raw"
  | "node"
  | "bytes";

export type TurbopackRuleConfigItem = {
  loaders?: TurbopackLoaderItem[];
  as?: string;
  /**
   * Controls the configured module type for matching resources.
   * Mapped to Turbopack's `ConfiguredModuleType`.
   */
  type?: TurbopackModuleType;
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

/**
 * Path rewrite: object form applies first matching regex→replacement;
 * function form receives path and returns new path (sync only).
 */
export type PathRewrite =
  | { [regexp: string]: string }
  | ((path: string) => string);

/**
 * Proxy rule for dev server HTTP/WS proxying.
 * Mostly aligned with webpack-dev-server / http-proxy-middleware options.
 */
export interface ProxyRule {
  /**
   * Path(s) to match for proxying.
   * When starts with `^`, will be treated as RegExp source.
   * Otherwise, simple prefix match is used.
   */
  context: string | string[];
  /** Target server URL. */
  target: string;
  /**
   * Path rewrite. Object: first matching regex is replaced (http-proxy-middleware style).
   * Function: (path) => new path.
   */
  pathRewrite?: PathRewrite;
  /**
   * Whether to change the Host header to match the target.
   * Defaults to true.
   */
  changeOrigin?: boolean;
  /**
   * Whether to verify the SSL certificate of the target.
   * Set to false to accept self-signed certificates.
   */
  secure?: boolean;
}

export type DevServerProxy = ProxyRule[];

/** Object style proxy options without `context`. */
export type ProxyOptions = Omit<ProxyRule, "context">;

export interface ResolveOptions {
  alias?: Record<string, string | string[] | Record<string, string | string[]>>;
  extensions?: string[];
}

export type ExternalType = "script" | "commonjs" | "esm" | "global" | "promise";

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

/**
 * Development server options (port, host, HTTPS, HMR).
 * Used by the dev server and by config resolution for startup options.
 */
export interface DevServerConfig {
  /** Enable Hot Module Replacement. */
  hot?: boolean;
  /** Port to listen on. */
  port?: number;
  /** Host to bind (e.g. localhost, 0.0.0.0). */
  host?: string;
  /** Use HTTPS; when true without a certificate, a self-signed cert may be generated. */
  https?: boolean;
  /** HTTP proxy rules for Hono dev server (HTTP only; no generic WS proxy in this layer). */
  proxy?: DevServerProxy;
}

export type CssChunkingConfig =
  | boolean
  | "strict"
  | "loose"
  | "graph"
  | {
      type: "strict" | "loose";
    }
  | {
      type: "graph";
      requestCost?: number;
      weightDistribution?: number;
    };

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
    /** URL prefix for chunks/assets. Use "runtime" for globalThis.publicPath or "auto" for current-script inference. */
    publicPath?: string;
    crossOriginLoading?: false | "anonymous" | "use-credentials";
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
    compress?:
      | boolean
      | {
          passes?: number;
          sequences?: number;
          keepClassnames?: boolean;
          keepFnames?: boolean;
        };
    minify?: boolean;
    /** Extract legal comments to `[file].LICENSE.txt` when minifying library output. */
    extractComments?: boolean;
    treeShaking?: boolean;
    splitChunks?: Record<
      "js" | "css",
      {
        minChunkSize?: number;
        maxChunkCountPerGroup?: number;
        maxMergeChunkSize?: number;
      }
    >;
    cssChunking?: CssChunkingConfig;
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
    autoCssModules?: boolean;
    cssModules?: {
      localIdentName?: string;
    };
    emotion?: boolean | EmotionOptions;
    postcss?: JSONValue;
    less?: {
      loader?: string;
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
    absoluteSourceFilename?: boolean;
  };
  stats?: boolean;
  reactCompiler?: ReactCompilerConfig;
  experimental?: {
    reactCompiler?: ReactCompilerConfig;
    swcPlugins?: [string, any][];
  };
  swcPlugins?: [string, any][];
  pluginRuntimeStrategy?: "workerThreads" | "childProcesses";
  persistentCaching?: boolean;
  /**
   * Controls memory eviction for the persistent Turbopack cache.
   * Defaults to "auto". Use false to disable eviction or "full" to evict
   * after every snapshot.
   */
  turbopackMemoryEviction?: boolean | "auto" | "full";
  nodePolyfill?: boolean;
  mdx?: MdxOptions;
  devServer?: DevServerConfig;
  server?: {
    /** Entry point for the server runtime (e.g. "src/server.ts") */
    entry?: ServerEntry;
    /** Additional named server entries emitted alongside the primary entry. */
    entries?: ServerEntryOptions[];
    output?: {
      /** Output path for server chunks, relative to project root. */
      path?: string;
      /** Entry chunk filename template. Supports [name]. */
      filename?: string;
      /** Non-entry chunk filename template. Supports [name] and [contenthash:N]. */
      chunkFilename?: string;
    };
    function?: {
      /** Module that exports `createServerReference(actionId, name)` for client-side proxy generation. */
      clientProxy?: string;
      /** Module that exports `registerServerReference(action, actionId, name)` for the server bundle. */
      serverRegister?: string;
    };
  };
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

export interface EmotionOptions {
  sourcemap?: boolean;
  labelFormat?: string;
  autoLabel?: "dev-only" | "always" | "never";
  importMap?: Record<string, any>;
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

  /**
   * Whether to watch the filesystem for file changes.
   */
  watch?: {
    enable?: boolean;
    pollIntervalMs?: number;
    /**
     * Paths to ignore when watching. Defaults to `node_modules`.
     * `!node_modules/<regex>` exempts matching dependency package names.
     */
    ignored?: string[];
    /**
     * Package name regular expressions for dependencies inside node_modules that should still be
     * watched when node_modules is ignored. Patterns are matched against package names such as
     * `rc-util` or `@rc-component/trigger`.
     *
     * @example
     * `["rc-.*", ".*cssinjs.*", "@rc-component/.*"]`
     */
    nodeModulesRegexes?: string[];
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
   * Whether to enable default utoopack tracing logs.
   * Defaults to true.
   */
  tracing?: boolean;

  /**
   * Absolute path for `@utoo/pack`.
   */
  packPath?: string;
}

/**
 * User config file shape (e.g. utoopack.config.mjs).
 * Bundler options (entry, output, define, ...) at top level,
 * plus optional CLI/runtime options (processEnv, watch, dev, ...).
 */
export type UserConfig = ConfigComplete &
  Partial<
    Pick<
      BundleOptions,
      "processEnv" | "watch" | "dev" | "buildId" | "tracing" | "packPath"
    >
  > & {
    rootPath?: string;
    projectPath?: string;
  };
