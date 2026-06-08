import {
  BundleOptions,
  ConfigComplete,
  ExternalConfig,
  HtmlConfig,
  TurbopackRuleConfigCollection,
  TurbopackRuleConfigItem,
} from "./config";

export interface WebpackPluginInstance {
  apply: (compiler: any) => void;
  [key: string]: any;
}

export interface WebpackRuleSetRule {
  test?: RegExp | string;
  use?: any;
  loader?: string;
  options?: any;
  oneOf?: WebpackRuleSetRule[];
  type?: string;
  generator?: any;
  parser?: any;
  sideEffects?: boolean;
  exclude?: any;
  include?: any;
  resourceQuery?: any;
  issuer?: any;
}

export interface WebpackModule {
  rules?: (WebpackRuleSetRule | "...")[] | WebpackRuleSetRule[];
}

export interface WebpackResolve {
  alias?:
    | Record<string, string | false | string[]>
    | Array<{ name: string; alias: string | false | string[] }>;
  extensions?: string[];
  modules?: string[];
  mainFields?: string[];
  conditionNames?: string[];
  fallback?: Record<string, string | false | string[]>;
  plugins?: any[];
}

export interface WebpackOutput {
  path?: string;
  publicPath?: string | ((...args: any[]) => string);
  crossOriginLoading?: false | "anonymous" | "use-credentials";
  filename?: string | ((...args: any[]) => string);
  chunkFilename?: string | ((...args: any[]) => string);
  cssFilename?: string | ((...args: any[]) => string);
  cssChunkFilename?: string | ((...args: any[]) => string);
  assetModuleFilename?: string | ((...args: any[]) => string);
  library?: any;
  libraryTarget?: string;
  globalObject?: string;
  chunkLoadingGlobal?: string;
  clean?: boolean | { keep?: any };
}

export interface WebpackOptimization {
  minimize?: boolean;
  minimizer?: any[];
  splitChunks?: any;
  runtimeChunk?: any;
  moduleIds?: string;
  chunkIds?: string;
  concatenateModules?: boolean;
  usedExports?: boolean | "global";
  mangleExports?: boolean | "deterministic" | "size";
}

export type WebpackEntry =
  | string
  | string[]
  | Record<
      string,
      | string
      | {
          import: string;
          library?: {
            type: string;
            name?: string | string[];
            export?: string | string[];
          };
        }
      | string[]
    >;

export type WebpackExternals =
  | string
  | RegExp
  | Record<string, string | string[] | object>
  | (string | RegExp | Record<string, string | string[] | object>)[];

export type WebpackExternalsType = "promise";
export type WebpackTarget = string | string[];
export type WebpackDevTool = string | boolean;
export type WebpackStats = string | boolean | object;
export type WebpackPlugins = (WebpackPluginInstance | any)[];
export type WebpackMode = "development" | "production" | "none";

export interface WebpackConfig {
  name?: string;
  entry?: WebpackEntry;
  mode?: WebpackMode;
  module?: WebpackModule;
  resolve?: WebpackResolve;
  externals?: WebpackExternals;
  externalsType?: WebpackExternalsType;
  output?: WebpackOutput;
  target?: WebpackTarget;
  devtool?: WebpackDevTool;
  optimization?: WebpackOptimization;
  plugins?: WebpackPlugins;
  stats?: WebpackStats;
  webpackMode: true;
  devServer?: any;
}

export function compatOptionsFromWebpack(
  webpackConfig: WebpackConfig,
): BundleOptions {
  const {
    entry,
    mode,
    module,
    resolve,
    externals,
    externalsType,
    output,
    target,
    devtool,
    optimization,
    plugins,
    stats,
  } = webpackConfig;

  const html = compatFromWebpackPlugin(plugins, compatHtml);
  const copy = compatFromWebpackPlugin(plugins, compatCopy);
  const outputCompat = compatOutput(output);
  if (outputCompat && !outputCompat.cssFilename) {
    outputCompat.cssFilename = compatFromWebpackPlugin(
      plugins,
      compatMiniCssExtract,
    );
  }
  if (outputCompat && copy) {
    (outputCompat as any).copy = copy;
  }
  const modeCompat = compatMode(mode);

  return {
    config: {
      entry: compatEntry(entry, html),
      mode: modeCompat,
      module: compatModule(module),
      resolve: compatResolve(resolve),
      externals: compatExternals(externals, externalsType),
      output: outputCompat,
      target: compatTarget(target),
      sourceMaps: compatSourceMaps(devtool),
      optimization: compatOptimization(optimization, modeCompat),
      define: compatFromWebpackPlugin(plugins, compatDefine),
      provider: compatFromWebpackPlugin(plugins, compatProvider),
      stats: compatStats(stats),
      devServer: compatDevServer((webpackConfig as any).devServer),
    } as ConfigComplete,
    buildId: webpackConfig.name,
  };
}

function compatMode(
  webpackMode: WebpackMode | undefined,
): "development" | "production" | undefined {
  return webpackMode
    ? webpackMode === "none"
      ? "production"
      : webpackMode
    : "production";
}

function compatEntry(
  webpackEntry: WebpackEntry | undefined,
  html?: HtmlConfig,
) {
  if (!webpackEntry) {
    return undefined;
  }
  const entry: ConfigComplete["entry"] = [];

  switch (typeof webpackEntry) {
    case "string":
      entry.push({ import: webpackEntry, html });
      break;
    case "object":
      if (Array.isArray(webpackEntry)) {
        webpackEntry.forEach((e) =>
          entry.push({
            import: e,
            html,
          }),
        );
      } else {
        Object.entries(webpackEntry).forEach(([k, v]) => {
          switch (typeof v) {
            case "string":
              entry.push({ name: k, import: v, html });
              break;
            case "object":
              if (!Array.isArray(v)) {
                switch (typeof v.import) {
                  case "string":
                    entry.push({
                      name: k,
                      import: v.import,
                      html,
                      library:
                        v.library?.type === "umd"
                          ? {
                              name:
                                typeof v.library.name === "string"
                                  ? v.library.name
                                  : undefined,
                              export:
                                typeof v.library.export === "string"
                                  ? [v.library.export]
                                  : v.library.export,
                            }
                          : undefined,
                    });
                    break;

                  default:
                    break;
                }
              } else {
                if (v.length === 0) {
                  throw "entry value items is empty";
                } else if (v.length === 1) {
                  entry.push({ name: k, import: v[0], html });
                } else {
                  throw "multi entry items for one entry not supported yet";
                }
              }
              break;
            default:
              throw "non string and non object entry path not supported yet";
          }
        });
      }
      break;
    case "function":
      throw new Error("functional entry not supported yet");
    default:
      throw new Error("entry config not compatible now");
  }

  return entry;
}

type MaybeWebpackPluginInstance = undefined | WebpackPluginInstance;

type WebpackPluginOptionsPicker<R> = ((
  p: MaybeWebpackPluginInstance,
) => R | undefined) & {
  pluginName: string;
};

function compatFromWebpackPlugin<R>(
  webpackPlugins: WebpackPlugins | undefined,
  picker: WebpackPluginOptionsPicker<R>,
): R | undefined {
  const plugin = webpackPlugins?.find(
    (p) =>
      p && typeof p === "object" && p.constructor.name === picker.pluginName,
  ) as MaybeWebpackPluginInstance;
  return picker(plugin);
}

compatHtml.pluginName = "HtmlWebpackPlugin";
function compatHtml(
  maybeWebpackPluginInstance: MaybeWebpackPluginInstance,
): HtmlConfig | undefined {
  if (!maybeWebpackPluginInstance) {
    return undefined;
  }
  return (
    (maybeWebpackPluginInstance as any).userOptions ||
    (maybeWebpackPluginInstance as any).options
  );
}

compatMiniCssExtract.pluginName = "MiniCssExtractPlugin";
function compatMiniCssExtract(
  maybeWebpackPluginInstance: MaybeWebpackPluginInstance,
): string | undefined {
  return (maybeWebpackPluginInstance as any)?.options?.filename;
}

compatCopy.pluginName = "CopyPlugin";
function compatCopy(
  maybeWebpackPluginInstance: MaybeWebpackPluginInstance,
): NonNullable<ConfigComplete["output"]>["copy"] | undefined {
  const patterns = (maybeWebpackPluginInstance as any)?.patterns;
  if (!Array.isArray(patterns)) return undefined;

  return patterns.map((pattern: any) => {
    if (typeof pattern === "string") return pattern;
    return {
      from: pattern.from,
      to: pattern.to,
    };
  });
}

compatDefine.pluginName = "DefinePlugin";
function compatDefine(maybeWebpackPluginInstance: MaybeWebpackPluginInstance) {
  const definitions = maybeWebpackPluginInstance?.definitions;
  if (!definitions || typeof definitions !== "object") {
    return definitions;
  }

  const processedDefinitions: Record<string, any> = {};
  for (const [key, value] of Object.entries(definitions)) {
    if (typeof value === "object" && value !== null) {
      processedDefinitions[key] = JSON.stringify(value);
    } else {
      processedDefinitions[key] = value;
    }
  }

  return processedDefinitions;
}

compatProvider.pluginName = "ProvidePlugin";
function compatProvider(
  maybeWebpackPluginInstance: MaybeWebpackPluginInstance,
): ConfigComplete["provider"] {
  const definitions = maybeWebpackPluginInstance?.definitions;
  if (!definitions || typeof definitions !== "object") {
    return undefined;
  }

  const provider: ConfigComplete["provider"] = {};
  for (const [key, value] of Object.entries(definitions)) {
    if (typeof value === "string") {
      // Simple module import: { $: 'jquery' }
      provider[key] = value;
    } else if (Array.isArray(value)) {
      if (value.length === 1) {
        // e.g. { process: ['process/browser'] } for default import
        provider[key] = value[0];
      } else if (value.length >= 2) {
        // Named export import: { Buffer: ['buffer', 'Buffer'] }
        provider[key] = [value[0], value[1]];
      }
    }
  }

  return Object.keys(provider).length > 0 ? provider : undefined;
}

function compatExternals(
  webpackExternals?: WebpackExternals,
  externalsType?: WebpackExternalsType,
): ConfigComplete["externals"] {
  if (!webpackExternals) {
    return undefined;
  }
  let externals: ConfigComplete["externals"] = {};
  switch (typeof webpackExternals) {
    case "string": {
      // Single string external: "lodash" -> { "lodash": "lodash" }
      externals[webpackExternals] = compatExternalValue(
        webpackExternals,
        externalsType,
      );
      break;
    }
    case "object": {
      if (Array.isArray(webpackExternals)) {
        // Array of externals: ["lodash", "react"] -> { "lodash": "lodash", "react": "react" }
        externals = webpackExternals.reduce(
          (acc, external) => {
            if (typeof external === "string") {
              acc![external] = compatExternalValue(external, externalsType);
            } else if (typeof external === "object" && external !== null) {
              Object.assign(acc!, compatExternals(external, externalsType));
            }
            return acc;
          },
          {} as ConfigComplete["externals"],
        );
      } else if (webpackExternals instanceof RegExp) {
        throw new Error("regex external not supported yet");
      } else {
        if ("byLayer" in webpackExternals) {
          throw new Error("by layer external item not supported yet");
        }
        Object.entries(webpackExternals).forEach(([key, value]) => {
          if (typeof value === "string") {
            // Check if it's a script type with shorthand syntax: "global@https://example.com/script.js"
            if (
              value.includes("@") &&
              (value.startsWith("script ") || value.includes("://"))
            ) {
              const match = value.match(/^(?:script\s+)?(.+?)@(.+)$/);
              if (match) {
                const [, globalName, scriptUrl] = match;
                // Use utoopack string format: "script globalName@url"
                externals![key] = `script ${globalName}@${scriptUrl}`;
              } else {
                externals![key] = value;
              }
            } else {
              // Simple string mapping: { "react": "React" }
              externals![key] = compatExternalValue(value, externalsType);
            }
          } else if (Array.isArray(value)) {
            // Array format handling
            if (value.length >= 2) {
              const [first, second] = value;

              // Check if it's a script type array: ["https://example.com/script.js", "GlobalName"]
              if (
                typeof first === "string" &&
                first.includes("://") &&
                typeof second === "string"
              ) {
                // Use utoopack object format for script
                externals![key] = {
                  root: second,
                  type: "script",
                  script: first,
                };
              } else if (
                typeof first === "string" &&
                typeof second === "string"
              ) {
                // Handle type prefix formats
                if (first.startsWith("commonjs")) {
                  externals![key] = `commonjs ${second}`;
                } else if (first === "module") {
                  externals![key] = `esm ${second}`;
                } else if (first === "promise") {
                  externals![key] = `promise ${second}`;
                } else if (
                  first === "var" ||
                  first === "global" ||
                  first === "window"
                ) {
                  externals![key] = second;
                } else if (first === "script") {
                  // Script type without URL in array format - treat as regular script prefix
                  externals![key] = `script ${second}`;
                } else {
                  externals![key] = `${first} ${second}`;
                }
              } else {
                externals![key] = compatExternalValue(
                  value[0] || key,
                  externalsType,
                );
              }
            } else {
              externals![key] = compatExternalValue(
                value[0] || key,
                externalsType,
              );
            }
          } else if (typeof value === "object" && value !== null) {
            // Object format: handle complex configurations
            const val = value as any;
            if ("root" in val || "commonjs" in val || "amd" in val) {
              // Standard webpack externals object format
              if (val.commonjs) {
                externals![key] = `commonjs ${val.commonjs}`;
              } else if (val.root) {
                externals![key] = compatExternalValue(val.root, externalsType);
              } else if (val.amd) {
                externals![key] = compatExternalValue(val.amd, externalsType);
              } else {
                externals![key] = compatExternalValue(key, externalsType);
              }
            } else {
              // Treat as utoopack specific configuration (might already be in correct format)
              externals![key] = value as ExternalConfig;
            }
          } else {
            // Fallback to key name
            externals![key] = compatExternalValue(key, externalsType);
          }
        });
      }
      break;
    }
    case "function": {
      throw new Error("functional external not supported yet");
    }
    default:
      break;
  }

  return externals;
}

function compatExternalValue(
  value: string,
  externalsType?: WebpackExternalsType,
): ExternalConfig {
  if (
    externalsType === "promise" &&
    !value.match(/^(commonjs|esm|script|promise)\s/)
  ) {
    return `promise ${value}`;
  }
  return value;
}

function compatModule(
  webpackModule: WebpackModule | undefined,
): ConfigComplete["module"] {
  if (!Array.isArray(webpackModule?.rules)) {
    return;
  }
  const moduleRules = {
    rules: webpackModule.rules.reduce(
      (acc, cur) => {
        if (typeof cur === "object" && cur) {
          // Normalize condition from test, include, exclude
          let ruleCondition: any = undefined;
          if (cur.test || cur.include || cur.exclude) {
            const conditions: any[] = [];
            if (cur.test) conditions.push({ path: cur.test });
            if (cur.include) conditions.push({ path: cur.include });
            if (cur.exclude) conditions.push({ not: { path: cur.exclude } });

            ruleCondition =
              conditions.length === 1 ? conditions[0] : { all: conditions };
          }

          // Determine the glob keys for the rules record
          // utoo usually uses extensions as keys like "*.svg"
          const extensions = cur.test?.toString().match(/\.(\w+)/g);
          const globKeys = extensions
            ? extensions.map((ext) => `*${ext}`)
            : ["*"];

          // Handle loaders
          const loaders =
            typeof cur.use === "string"
              ? [{ loader: cur.use, options: {} }]
              : Array.isArray(cur.use)
                ? cur.use.map((use) =>
                    typeof use === "string"
                      ? { loader: use, options: {} }
                      : {
                          loader: (use as any).loader,
                          options: (use as any).options || {},
                        },
                  )
                : cur.loader
                  ? [
                      {
                        loader: cur.loader,
                        options: cur.options || {},
                      },
                    ]
                  : [];

          const ruleItem: TurbopackRuleConfigItem = {
            loaders,
            condition: ruleCondition,
            as:
              cur.type === "asset" ||
              cur.type === "asset/resource" ||
              cur.type === "asset/inline" ||
              cur.type === "asset/source" ||
              (loaders.length > 0 &&
                !globKeys.some(
                  (k) => k === "*.js" || k === "*.ts" || k === "*.tsx",
                ))
                ? "*.js"
                : undefined,
          };

          for (const key of globKeys) {
            const existing = acc[key];
            if (!existing) {
              acc[key] = ruleItem;
            } else {
              // If already exists, turn into a collection array
              if (Array.isArray(existing)) {
                existing.push(ruleItem);
              } else {
                acc[key] = [existing as any, ruleItem];
              }
            }
          }
        }
        return acc;
      },
      {} as Record<string, TurbopackRuleConfigCollection>,
    ),
  };

  return moduleRules;
}

function compatResolve(
  webpackResolve: WebpackResolve | undefined,
): ConfigComplete["resolve"] {
  if (!webpackResolve) {
    return;
  }
  const { alias, extensions } = webpackResolve;
  return {
    alias: alias
      ? Array.isArray(alias)
        ? alias.reduce(
            (acc, cur) => Object.assign(acc, { [cur.name]: cur.alias }),
            {},
          )
        : Object.entries(alias).reduce(
            (acc, [k, v]) => {
              if (typeof v === "string") {
                // Handle alias keys ending with $ by removing the $
                const aliasKey = k.endsWith("$") ? k.slice(0, -1) : k;
                acc[aliasKey] = v;
              } else {
                throw new Error("non string alias value not supported yet");
              }
              return acc;
            },
            {} as Record<string, any>,
          )
      : undefined,
    extensions,
  };
}

function normalizeWebpackHash(
  filename: string | undefined,
): string | undefined {
  if (!filename) return filename;
  // Replace [hash] and [chunkhash] with [contenthash] as utoo only supports contenthash
  // Also remove [ext] as it is automatically appended in utoopack
  return filename
    .replace(/\[(?:hash|chunkhash)(?::(\d+))?\]/g, "[contenthash:$1]")
    .replace(/\[ext\]/g, "");
}

function compatOutput(
  webpackOutput: WebpackOutput | undefined,
): ConfigComplete["output"] {
  for (const field of [
    "filename",
    "chunkFilename",
    "publicPath",
    "cssFilename",
    "cssChunkFilename",
    "assetModuleFilename",
  ] as const) {
    if (webpackOutput?.[field] && typeof webpackOutput[field] !== "string") {
      throw new Error(`non string output ${field} not supported yet`);
    }
  }

  return {
    path: webpackOutput?.path,
    filename: normalizeWebpackHash(
      webpackOutput?.filename as string | undefined,
    ),
    chunkFilename: normalizeWebpackHash(
      webpackOutput?.chunkFilename as string | undefined,
    ),
    cssFilename: normalizeWebpackHash(
      webpackOutput?.cssFilename as string | undefined,
    ),
    cssChunkFilename: normalizeWebpackHash(
      webpackOutput?.cssChunkFilename as string | undefined,
    ),
    assetModuleFilename: normalizeWebpackHash(
      webpackOutput?.assetModuleFilename as string | undefined,
    ),
    clean: !!webpackOutput?.clean,
    publicPath: webpackOutput?.publicPath as string | undefined,
    crossOriginLoading: webpackOutput?.crossOriginLoading,
    chunkLoadingGlobal: webpackOutput?.chunkLoadingGlobal,
  };
}

function compatTarget(
  webpackTarget: WebpackTarget | undefined,
): ConfigComplete["target"] {
  return webpackTarget
    ? Array.isArray(webpackTarget)
      ? webpackTarget.join(" ")
      : webpackTarget
    : undefined;
}

function compatSourceMaps(
  webpackSourceMaps: WebpackDevTool | undefined,
): ConfigComplete["sourceMaps"] {
  return !!webpackSourceMaps;
}

function compatOptimization(
  webpackOptimization: WebpackOptimization | undefined,
  mode: ConfigComplete["mode"],
): ConfigComplete["optimization"] {
  const productionDefault = mode === "production" ? true : undefined;

  if (!webpackOptimization) {
    if (mode === "production") {
      return {
        concatenateModules: true,
        removeUnusedExports: true,
        removeUnusedImports: true,
      };
    }
    return;
  }
  const { moduleIds, minimize, concatenateModules, usedExports } =
    webpackOptimization;
  const enableWebpackUsedExports =
    typeof usedExports === "undefined" ? undefined : usedExports !== false;
  return {
    moduleIds:
      moduleIds === "named"
        ? "named"
        : moduleIds === "deterministic"
          ? "deterministic"
          : undefined,
    noMangling: webpackOptimization.mangleExports === false,
    minify: minimize,
    concatenateModules: concatenateModules ?? productionDefault,
    treeShaking: false,
    removeUnusedExports: enableWebpackUsedExports ?? productionDefault,
    removeUnusedImports: enableWebpackUsedExports ?? productionDefault,
  };
}

function compatStats(
  webpackStats: WebpackStats | undefined,
): ConfigComplete["stats"] {
  return !!webpackStats;
}

function compatDevServer(devServer: any): ConfigComplete["devServer"] {
  if (!devServer) return undefined;
  const result: NonNullable<ConfigComplete["devServer"]> = {};
  if (typeof devServer.hot !== "undefined") result.hot = !!devServer.hot;
  if (typeof devServer.port !== "undefined") {
    const p = Number(devServer.port);
    if (!Number.isNaN(p)) result.port = p;
  }
  if (typeof devServer.host !== "undefined") result.host = devServer.host;
  if (typeof devServer.https !== "undefined") result.https = !!devServer.https;
  return result;
}
