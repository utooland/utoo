import {
  BundleOptions,
  ConfigComplete,
  ExternalConfig,
  HtmlConfig,
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
  filename?: string | ((...args: any[]) => string);
  chunkFilename?: string | ((...args: any[]) => string);
  assetModuleFilename?: string;
  library?: any;
  libraryTarget?: string;
  globalObject?: string;
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
    output,
    target,
    devtool,
    optimization,
    plugins,
    stats,
  } = webpackConfig;

  return {
    config: {
      entry: compatEntry(entry),
      mode: compatMode(mode),
      module: compatModule(module),
      resolve: compatResolve(resolve),
      externals: compatExternals(externals),
      output: compatOutput(output),
      target: compatTarget(target),
      platform: compatPlatform(target),
      sourceMaps: compatSourceMaps(devtool),
      optimization: compatOptimization(optimization),
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

function compatEntry(webpackEntry: WebpackEntry | undefined) {
  if (!webpackEntry) {
    return undefined;
  }
  const entry: ConfigComplete["entry"] = [];

  switch (typeof webpackEntry) {
    case "string":
      entry.push({ import: webpackEntry });
      break;
    case "object":
      if (Array.isArray(webpackEntry)) {
        webpackEntry.forEach((e) =>
          entry.push({
            import: e,
          }),
        );
      } else {
        Object.entries(webpackEntry).forEach(([k, v]) => {
          switch (typeof v) {
            case "string":
              entry.push({ name: k, import: v });
              break;
            case "object":
              if (!Array.isArray(v)) {
                switch (typeof v.import) {
                  case "string":
                    entry.push({
                      name: k,
                      import: v.import,
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
                  entry.push({ name: k, import: v[0] });
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
      throw "functional entry not supported yet";
    default:
      throw "entry config not compatible now";
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
): ConfigComplete["externals"] {
  if (!webpackExternals) {
    return undefined;
  }
  let externals: ConfigComplete["externals"] = {};
  switch (typeof webpackExternals) {
    case "string": {
      // Single string external: "lodash" -> { "lodash": "lodash" }
      externals[webpackExternals] = webpackExternals;
      break;
    }
    case "object": {
      if (Array.isArray(webpackExternals)) {
        // Array of externals: ["lodash", "react"] -> { "lodash": "lodash", "react": "react" }
        externals = webpackExternals.reduce(
          (acc, external) => {
            if (typeof external === "string") {
              acc![external] = external;
            } else if (typeof external === "object" && external !== null) {
              Object.assign(acc!, compatExternals(external));
            }
            return acc;
          },
          {} as ConfigComplete["externals"],
        );
      } else if (webpackExternals instanceof RegExp) {
        throw "regex external not supported yet";
      } else {
        if ("byLayer" in webpackExternals) {
          throw "by layer external item not supported yet";
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
              externals![key] = value;
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
                externals![key] = value[0] || key;
              }
            } else {
              externals![key] = value[0] || key;
            }
          } else if (typeof value === "object" && value !== null) {
            // Object format: handle complex configurations
            const val = value as any;
            if ("root" in val || "commonjs" in val || "amd" in val) {
              // Standard webpack externals object format
              if (val.commonjs) {
                externals![key] = `commonjs ${val.commonjs}`;
              } else if (val.root) {
                externals![key] = val.root;
              } else if (val.amd) {
                externals![key] = val.amd;
              } else {
                externals![key] = key;
              }
            } else {
              // Treat as utoopack specific configuration (might already be in correct format)
              externals![key] = value as ExternalConfig;
            }
          } else {
            // Fallback to key name
            externals![key] = key;
          }
        });
      }
      break;
    }
    case "function": {
      throw "functional external not supported yet";
    }
    default:
      break;
  }

  return externals;
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
        switch (typeof cur) {
          case "object":
            if (cur) {
              let condition = cur.test?.toString().match(/(\.\w+)/)?.[1];
              if (condition) {
                Object.assign(acc, {
                  ["*" + condition]: <TurbopackRuleConfigItem>{
                    loaders:
                      typeof cur.use === "string"
                        ? [cur.use]
                        : typeof cur?.use === "object"
                          ? Array.isArray(cur.use)
                            ? cur.use.map((use) =>
                                typeof use === "string"
                                  ? { loader: use, options: {} }
                                  : {
                                      loader: (<any>use).loader,
                                      options: (<any>use).options || {},
                                    },
                              )
                            : [
                                {
                                  loader: cur.loader!,
                                  options: cur.options || {},
                                },
                              ]
                          : [],
                    as: "*.js",
                  },
                });
              }
            }
            break;
          default:
            break;
        }

        return acc;
      },
      {} as Record<string, TurbopackRuleConfigItem>,
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
        : Object.entries(alias).reduce((acc, [k, v]) => {
            if (typeof v === "string") {
              // Handle alias keys ending with $ by removing the $
              const aliasKey = k.endsWith("$") ? k.slice(0, -1) : k;
              Object.assign(acc, { [aliasKey]: v });
            } else {
              throw "non string alias value not supported yet";
            }
            return acc;
          }, {})
      : undefined,
    extensions,
  };
}

function compatOutput(
  webpackOutput: WebpackOutput | undefined,
): ConfigComplete["output"] {
  if (webpackOutput?.filename && typeof webpackOutput.filename !== "string") {
    throw "non string output filename not supported yet";
  }
  if (
    webpackOutput?.chunkFilename &&
    typeof webpackOutput.chunkFilename !== "string"
  ) {
    throw "non string output chunkFilename not supported yet";
  }

  if (
    webpackOutput?.publicPath &&
    typeof webpackOutput.publicPath !== "string"
  ) {
    throw "non string output publicPath not supported yet";
  }
  return {
    path: webpackOutput?.path,
    filename: webpackOutput?.filename as string | undefined,
    chunkFilename: webpackOutput?.chunkFilename as string | undefined,
    clean: !!webpackOutput?.clean,
    publicPath: webpackOutput?.publicPath as string | undefined,
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

function compatPlatform(
  webpackTarget: WebpackTarget | undefined,
): ConfigComplete["platform"] {
  if (!webpackTarget) return undefined;
  const targets = Array.isArray(webpackTarget) ? webpackTarget : [webpackTarget];
  return targets.some((t) => t === "node") ? "node" : undefined;
}

function compatSourceMaps(
  webpackSourceMaps: WebpackDevTool | undefined,
): ConfigComplete["sourceMaps"] {
  return !!webpackSourceMaps;
}

function compatOptimization(
  webpackOptimization: WebpackOptimization | undefined,
): ConfigComplete["optimization"] {
  if (!webpackOptimization) {
    return;
  }
  const { moduleIds, minimize, concatenateModules } = webpackOptimization;
  return {
    moduleIds:
      moduleIds === "named"
        ? "named"
        : moduleIds === "deterministic"
          ? "deterministic"
          : undefined,
    minify: minimize,
    concatenateModules,
  };
}

function compatStats(
  webpackStats: WebpackStats | undefined,
): ConfigComplete["sourceMaps"] {
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
