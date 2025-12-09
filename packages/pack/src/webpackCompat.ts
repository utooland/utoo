import path from "path";
import { config } from "process";
import type webpack from "webpack";
import {
  BundleOptions,
  ConfigComplete,
  ExternalConfig,
  TurbopackRuleConfigItem,
} from "./types";

export function readWebpackConfig(projectPath?: string, rootPath?: string) {
  const projectPathOutOfRoot =
    projectPath === undefined
      ? process.cwd()
      : path.join(rootPath ?? "", projectPath);
  const configPath = path.join(projectPathOutOfRoot, "webpack.config.js");
  return require(configPath);
}

export type WebpackConfig = Partial<
  Pick<
    webpack.Configuration,
    | "name"
    | "entry"
    | "mode"
    | "module"
    | "resolve"
    | "externals"
    | "output"
    | "target"
    | "devtool"
    | "optimization"
    | "plugins"
    | "stats"
  >
> & {
  webpackMode: true;
};

export function compatOptionsFromWebpack(
  webpackConfig: WebpackConfig,
  projectPath?: string,
  rootPath?: string,
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
  } = webpackConfig.entry
    ? webpackConfig
    : readWebpackConfig(projectPath, rootPath);

  return {
    config: {
      entry: compatEntry(entry),
      mode: compatMode(mode),
      module: compatModule(module),
      resolve: compatResolve(resolve),
      externals: compatExternals(externals),
      output: compatOutput(output),
      target: compatTarget(target),
      sourceMaps: compatSourceMaps(devtool),
      optimization: compatOptimization(optimization),
      define: compatFromWebpackPlugin(plugins, compatDefine),
      stats: compatStats(stats),
    },
    buildId: webpackConfig.name,
  };
}

function compatMode(
  webpackMode: webpack.Configuration["mode"],
): "development" | "production" | undefined {
  return webpackMode
    ? webpackMode === "none"
      ? "production"
      : webpackMode
    : "production";
}

function compatEntry(webpackEntry: WebpackConfig["entry"]) {
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

type MaybeWebpackPluginInstance = undefined | webpack.WebpackPluginInstance;

type WebpackPluginOptionsPicker<R> = ((p: MaybeWebpackPluginInstance) => R) & {
  pluginName: string;
};

function compatFromWebpackPlugin<R>(
  webpackPlugins: webpack.Configuration["plugins"],
  picker: WebpackPluginOptionsPicker<R>,
): R {
  const plugin = webpackPlugins?.find(
    (p) =>
      p && typeof p === "object" && p.constructor.name === picker.pluginName,
  ) as MaybeWebpackPluginInstance;
  return picker(plugin);
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

function compatExternals(
  webpackExternals?: webpack.Configuration["externals"],
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
                // Use utoo-pack string format: "script globalName@url"
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
                // Use utoo-pack object format for script
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
            if ("root" in value || "commonjs" in value || "amd" in value) {
              // Standard webpack externals object format
              if (value.commonjs) {
                externals![key] = `commonjs ${value.commonjs}`;
              } else if (value.root) {
                externals![key] = value.root;
              } else if (value.amd) {
                externals![key] = value.amd;
              } else {
                externals![key] = key;
              }
            } else {
              // Treat as utoo-pack specific configuration (might already be in correct format)
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
  webpackModule: webpack.Configuration["module"],
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
  webpackResolve: webpack.Configuration["resolve"],
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
  webpackOutput: webpack.Configuration["output"],
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
  webpackTarget: webpack.Configuration["target"],
): ConfigComplete["target"] {
  return webpackTarget
    ? Array.isArray(webpackTarget)
      ? webpackTarget.join(" ")
      : webpackTarget
    : undefined;
}

function compatSourceMaps(
  webpackSourceMaps: webpack.Configuration["devtool"],
): ConfigComplete["sourceMaps"] {
  return !!webpackSourceMaps;
}

function compatOptimization(
  webpackOptimization: webpack.Configuration["optimization"],
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
  webpackStats: webpack.Configuration["stats"],
): ConfigComplete["sourceMaps"] {
  return !!webpackStats;
}
