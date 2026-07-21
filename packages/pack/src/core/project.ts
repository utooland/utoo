import { resolve } from "path";
import { isDeepStrictEqual } from "util";
import type {
  HmrIdentifiers,
  NapiPartialProjectOptions,
  NapiProjectOptions,
  NapiUpdateMessage,
  NapiWrittenEndpoint,
  StackFrame,
  TurbopackInternalErrorOpts,
} from "../binding";
import * as binding from "../binding";
import {
  ConfigComplete,
  EmotionOptions,
  TurbopackLoaderBuiltinCondition,
  TurbopackLoaderItem,
  TurbopackRuleCondition,
  TurbopackRuleConfigCollection,
  TurbopackRuleConfigItem,
} from "../config/types";
import { getPackPath, rustifyEnv } from "../utils/common";
import { normalizePath } from "../utils/normalizePath";
import { runLoaderWorkerPool } from "./loaderWorkerPool";
import {
  Endpoint,
  Project,
  ProjectOptions,
  RawEntrypoints,
  Update,
} from "./types";

/**
 * An error caused by a bug in Turbopack, and not the user's code (e.g. a Rust panic). These should
 * be written to a log file and details should not be shown to the user.
 *
 * These are constructed in Turbopack by calling `throwTurbopackInternalError`.
 */
export class TurbopackInternalError extends Error {
  name = "TurbopackInternalError";
  location: string | undefined;

  constructor({ message, anonymizedLocation }: TurbopackInternalErrorOpts) {
    super(message);
    this.location = anonymizedLocation;
  }
}

/**
 * A helper used by the napi Rust entrypoints to construct and throw a `TurbopackInternalError`.
 */
function throwTurbopackInternalError(
  conversionError: Error | null,
  opts: TurbopackInternalErrorOpts,
): never {
  if (conversionError != null) {
    throw new Error(
      "NAPI type conversion error in throwTurbopackInternalError",
      { cause: conversionError },
    );
  }
  throw new TurbopackInternalError(opts);
}

async function withErrorCause<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (nativeError: any) {
    throw new TurbopackInternalError({
      message: nativeError?.message ?? String(nativeError),
    });
  }
}

function normalizeEmotionConfig(
  emotion: EmotionOptions | boolean | undefined,
  isDev: boolean,
): EmotionOptions | undefined {
  if (emotion === undefined || emotion === false) {
    return undefined;
  }

  const defaults = {
    sourcemap: isDev,
    autoLabel: isDev ? ("always" as const) : ("never" as const),
  };

  return emotion === true ? defaults : { ...defaults, ...emotion };
}

function normalizeStyles(
  styles: ConfigComplete["styles"],
  isDev: boolean,
): ConfigComplete["styles"] {
  if (!styles) {
    return styles;
  }

  return {
    ...styles,
    emotion: normalizeEmotionConfig(styles.emotion, isDev),
  };
}

// Align with next.js turbopack default optimizePackageImports config:
// https://nextjs.org/docs/app/api-reference/config/next-config-js/optimizePackageImports
const DEFAULT_OPTIMIZE_PACKAGE_IMPORTS = [
  "lucide-react",
  "date-fns",
  "lodash-es",
  "ramda",
  "react-bootstrap",
  "ahooks",
  "@ant-design/icons",
  "@headlessui/react",
  "@headlessui-float/react",
  "@heroicons/react/20/solid",
  "@heroicons/react/24/solid",
  "@heroicons/react/24/outline",
  "@visx/visx",
  "@tremor/react",
  "rxjs",
  "@mui/material",
  "@mui/icons-material",
  "recharts",
  "react-use",
  "effect",
  "@effect/schema",
  "@effect/platform",
  "@effect/platform-node",
  "@effect/platform-browser",
  "@effect/platform-bun",
  "@effect/sql",
  "@effect/sql-mssql",
  "@effect/sql-mysql2",
  "@effect/sql-pg",
  "@effect/sql-sqlite-node",
  "@effect/sql-sqlite-bun",
  "@effect/sql-sqlite-wasm",
  "@effect/sql-sqlite-react-native",
  "@effect/rpc",
  "@effect/rpc-http",
  "@effect/typeclass",
  "@effect/experimental",
  "@effect/opentelemetry",
  "@material-ui/core",
  "@material-ui/icons",
  "@tabler/icons-react",
  "mui-core",
  "react-icons/ai",
  "react-icons/bi",
  "react-icons/bs",
  "react-icons/cg",
  "react-icons/ci",
  "react-icons/di",
  "react-icons/fa",
  "react-icons/fa6",
  "react-icons/fc",
  "react-icons/fi",
  "react-icons/gi",
  "react-icons/go",
  "react-icons/gr",
  "react-icons/hi",
  "react-icons/hi2",
  "react-icons/im",
  "react-icons/io",
  "react-icons/io5",
  "react-icons/lia",
  "react-icons/lib",
  "react-icons/lu",
  "react-icons/md",
  "react-icons/pi",
  "react-icons/ri",
  "react-icons/rx",
  "react-icons/si",
  "react-icons/sl",
  "react-icons/tb",
  "react-icons/tfi",
  "react-icons/ti",
  "react-icons/vsc",
  "react-icons/wi",
];

function mergePackageImports(packageImports: string[] | undefined): string[] {
  const defaultPackageImports =
    process.env.UTOO_DISABLE_DEFAULT_PACKAGE_IMPORTS === "1"
      ? []
      : DEFAULT_OPTIMIZE_PACKAGE_IMPORTS;

  return [...new Set([...(packageImports ?? []), ...defaultPackageImports])];
}

function normalizeOptimization(
  optimization: ConfigComplete["optimization"],
): NonNullable<ConfigComplete["optimization"]> {
  return {
    ...(optimization ?? {}),
    packageImports: mergePackageImports(optimization?.packageImports),
  };
}

export async function serializeConfig(
  config: ConfigComplete,
  isDev: boolean,
): Promise<string> {
  const configSerializable = {
    ...config,
    styles: normalizeStyles(config.styles, isDev),
    optimization: normalizeOptimization(config.optimization),
  };

  if (configSerializable.entry) {
    configSerializable.entry = configSerializable.entry.map((entry) => {
      const { html, ...rest } = entry;
      return rest;
    });
  }

  {
    const { modularizeImports } = configSerializable.optimization;

    if (modularizeImports) {
      configSerializable.optimization.modularizeImports = Object.fromEntries(
        Object.entries<any>(modularizeImports).map(([mod, config]) => [
          mod,
          {
            ...config,
            transform:
              typeof config.transform === "string"
                ? config.transform
                : Object.entries(config.transform).map(([key, value]) => [
                    key,
                    value,
                  ]),
          },
        ]),
      );
    }
  }

  if (configSerializable.module && configSerializable.module.rules) {
    configSerializable.module.rules = serializeModuleRules(
      configSerializable.module.rules,
    );
  }

  return JSON.stringify(configSerializable, null, 2);
}

type SerializedRuleCondition =
  | { all: SerializedRuleCondition[] }
  | { any: SerializedRuleCondition[] }
  | { not: SerializedRuleCondition }
  | TurbopackLoaderBuiltinCondition
  | {
      path?:
        | { type: "regex"; value: { source: string; flags: string } }
        | { type: "glob"; value: string };
      content?: { source: string; flags: string };
      query?:
        | { type: "regex"; value: { source: string; flags: string } }
        | { type: "constant"; value: string };
      contentType?:
        | { type: "regex"; value: { source: string; flags: string } }
        | { type: "glob"; value: string };
    };

// converts regexes to a `RegexComponents` object so that it can be JSON-serialized when passed to
// Turbopack
function serializeRuleCondition(
  cond: TurbopackRuleCondition,
): SerializedRuleCondition {
  function regexComponents(regex: RegExp) {
    return {
      source: regex.source,
      flags: regex.flags,
    };
  }

  if (typeof cond === "string") {
    return cond;
  } else if ("all" in cond) {
    return { ...cond, all: cond.all.map(serializeRuleCondition) };
  } else if ("any" in cond) {
    return { ...cond, any: cond.any.map(serializeRuleCondition) };
  } else if ("not" in cond) {
    return { ...cond, not: serializeRuleCondition(cond.not) };
  } else {
    return {
      ...cond,
      path:
        cond.path == null
          ? undefined
          : cond.path instanceof RegExp
            ? {
                type: "regex",
                value: regexComponents(cond.path),
              }
            : { type: "glob", value: cond.path },
      content: cond.content && regexComponents(cond.content),
      query:
        cond.query == null
          ? undefined
          : cond.query instanceof RegExp
            ? {
                type: "regex",
                value: regexComponents(cond.query),
              }
            : { type: "constant", value: cond.query },
      contentType:
        cond.contentType == null
          ? undefined
          : cond.contentType instanceof RegExp
            ? {
                type: "regex",
                value: regexComponents(cond.contentType),
              }
            : { type: "glob", value: cond.contentType },
    };
  }
}

// Note: Returns an updated `turbopackRules` with serialized conditions. Does not mutate in-place.
function serializeModuleRules(
  turbopackRules: Record<string, TurbopackRuleConfigCollection>,
): Record<string, any> {
  const serializedRules: Record<string, any> = {};
  for (const [glob, rule] of Object.entries(turbopackRules)) {
    if (Array.isArray(rule)) {
      serializedRules[glob] = rule.map((item) => {
        if (
          typeof item !== "string" &&
          ("loaders" in item || "type" in item || "condition" in item)
        ) {
          return serializeConfigItem(item as TurbopackRuleConfigItem, glob);
        } else {
          checkLoaderItem(item as TurbopackLoaderItem, glob);
          return item;
        }
      });
    } else {
      serializedRules[glob] = serializeConfigItem(rule, glob);
    }
  }

  return serializedRules;

  function serializeConfigItem(
    rule: TurbopackRuleConfigItem,
    glob: string,
  ): any {
    if (!rule) return rule;
    if (rule.loaders) {
      for (const item of rule.loaders) {
        checkLoaderItem(item, glob);
      }
    }
    let serializedRule: any = rule;
    if (rule.condition != null) {
      serializedRule = {
        ...rule,
        condition: serializeRuleCondition(rule.condition),
      };
    }
    return serializedRule;
  }

  function checkLoaderItem(loaderItem: TurbopackLoaderItem, glob: string) {
    if (
      typeof loaderItem !== "string" &&
      !isDeepStrictEqual(loaderItem, JSON.parse(JSON.stringify(loaderItem)))
    ) {
      throw new Error(
        `loader ${loaderItem.loader} for match "${glob}" does not have serializable options. ` +
          "Ensure that options passed are plain JavaScript objects and values.",
      );
    }
  }
}

async function rustifyPartialProjectOptions(
  options: Partial<ProjectOptions>,
): Promise<NapiPartialProjectOptions> {
  return {
    ...options,
    rootPath: normalizePathOption(options.rootPath),
    projectPath: normalizePathOption(options.projectPath),
    packPath: normalizePathOption(options.packPath),
    config:
      options.config && (await serializeConfig(options.config, !!options.dev)),
    processEnv: options.processEnv && rustifyEnv(options.processEnv),
    watch: options.watch && {
      ...options.watch,
      enable: options.watch.enable ?? false,
    },
  };
}

type NativeFunction<T> = (
  callback: (err: Error, value: T) => void,
) => Promise<{ __napiType: "RootTask" }>;

function normalizePathOption(pathLike: string | undefined): string | undefined {
  if (pathLike === undefined) return undefined;
  return normalizePath(pathLike);
}

async function rustifyProjectOptions(
  options: Required<ProjectOptions>,
): Promise<NapiProjectOptions> {
  return {
    ...options,
    rootPath: normalizePath(options.rootPath),
    projectPath: normalizePath(options.projectPath),
    packPath: normalizePath(options.packPath),
    config: await serializeConfig(options.config, options.dev),
    processEnv: rustifyEnv(options.processEnv ?? {}),
    watch: {
      ...options.watch,
      enable: options.watch.enable ?? false,
    },
  };
}

export function projectFactory() {
  const cancel = new (class Cancel extends Error {})();

  function subscribe<T>(
    useBuffer: boolean,
    nativeFunction:
      | NativeFunction<T>
      | ((callback: (err: Error, value: T) => void) => Promise<void>),
  ): AsyncIterableIterator<T> {
    type BufferItem =
      | { err: Error; value: undefined }
      | { err: undefined; value: T };
    // A buffer of produced items. This will only contain values if the
    // consumer is slower than the producer.
    let buffer: BufferItem[] = [];
    // A deferred value waiting for the next produced item. This will only
    // exist if the consumer is faster than the producer.
    let waiting:
      | {
          resolve: (value: T) => void;
          reject: (error: Error) => void;
        }
      | undefined;
    let canceled = false;

    // The native function will call this every time it emits a new result. We
    // either need to notify a waiting consumer, or buffer the new result until
    // the consumer catches up.
    function emitResult(err: Error | undefined, value: T | undefined) {
      if (waiting) {
        let { resolve, reject } = waiting;
        waiting = undefined;
        if (err) reject(err);
        else resolve(value!);
      } else {
        const item = { err, value } as BufferItem;
        if (useBuffer) buffer.push(item);
        else buffer[0] = item;
      }
    }

    async function* createIterator() {
      const task = await withErrorCause<{ __napiType: "RootTask" } | void>(() =>
        nativeFunction(emitResult),
      );
      try {
        while (!canceled) {
          if (buffer.length > 0) {
            const item = buffer.shift()!;
            if (item.err) throw item.err;
            yield item.value;
          } else {
            // eslint-disable-next-line no-loop-func
            yield new Promise<T>((resolve, reject) => {
              waiting = { resolve, reject };
            });
          }
        }
      } catch (e) {
        if (e === cancel) return;
        if (e instanceof Error) {
          throw new TurbopackInternalError({ message: e.message });
        }
        throw e;
      } finally {
        if (task) {
          binding.rootTaskDispose(task);
        }
      }
    }

    const iterator = createIterator();
    iterator.return = async () => {
      canceled = true;
      if (waiting) waiting.reject(cancel);
      return { value: undefined, done: true } as IteratorReturnResult<never>;
    };
    return iterator;
  }

  class ProjectImpl implements Project {
    readonly _nativeProject: { __napiType: "Project" };

    constructor(nativeProject: { __napiType: "Project" }) {
      this._nativeProject = nativeProject;
      if (typeof binding.registerWorkerScheduler === "function") {
        runLoaderWorkerPool(binding, resolve(getPackPath(), "./binding.js"));
      }
    }

    async update(options: Partial<ProjectOptions>) {
      await withErrorCause(async () =>
        binding.projectUpdate(
          this._nativeProject,
          await rustifyPartialProjectOptions(options),
        ),
      );
    }

    async writeAllEntrypointsToDisk(): Promise<
      TurbopackResult<RawEntrypoints>
    > {
      return await withErrorCause(async () => {
        const napiEndpoints = (await binding.projectWriteAllEntrypointsToDisk(
          this._nativeProject,
        )) as TurbopackResult<{ __napiType: "Endpoint" }>;

        return napiEntrypointsToRawEntrypoints(napiEndpoints);
      });
    }

    entrypointsSubscribe() {
      type NapiEndpoint = { __napiType: "Endpoint" };

      type NapiEntrypoints = {
        apps?: NapiEndpoint[];
        libraries?: NapiEndpoint[];
      };

      const subscription = subscribe<TurbopackResult<NapiEntrypoints>>(
        false,
        async (callback) =>
          binding.projectEntrypointsSubscribe(this._nativeProject, callback),
      );
      return (async function* () {
        for await (const entrypoints of subscription) {
          yield napiEntrypointsToRawEntrypoints(entrypoints);
        }
      })();
    }

    hmrEvents(identifier: string, expectedVersion?: string) {
      return subscribe<TurbopackResult<Update>>(true, async (callback) =>
        binding.projectHmrEvents(
          this._nativeProject,
          identifier,
          callback,
          expectedVersion,
        ),
      );
    }

    hmrIdentifiersSubscribe() {
      return subscribe<TurbopackResult<HmrIdentifiers>>(
        false,
        async (callback) =>
          binding.projectHmrIdentifiersSubscribe(this._nativeProject, callback),
      );
    }

    traceSource(
      stackFrame: StackFrame,
      currentDirectoryFileUrl: string,
    ): Promise<StackFrame | null> {
      return binding.projectTraceSource(
        this._nativeProject,
        stackFrame,
        currentDirectoryFileUrl,
      );
    }

    getSourceForAsset(filePath: string): Promise<string | null> {
      return binding.projectGetSourceForAsset(this._nativeProject, filePath);
    }

    getSourceMap(filePath: string): Promise<string | null> {
      return binding.projectGetSourceMap(this._nativeProject, filePath);
    }

    getSourceMapSync(filePath: string): string | null {
      return binding.projectGetSourceMapSync(this._nativeProject, filePath);
    }

    updateInfoSubscribe(aggregationMs: number) {
      return subscribe<TurbopackResult<NapiUpdateMessage>>(
        true,
        async (callback) =>
          binding.projectUpdateInfoSubscribe(
            this._nativeProject,
            aggregationMs,
            callback,
          ),
      );
    }

    shutdown(): Promise<void> {
      return binding.projectShutdown(this._nativeProject);
    }

    onExit(): Promise<void> {
      return binding.projectOnExit(this._nativeProject);
    }
  }

  class EndpointImpl implements Endpoint {
    readonly _nativeEndpoint: { __napiType: "Endpoint" };

    constructor(nativeEndpoint: { __napiType: "Endpoint" }) {
      this._nativeEndpoint = nativeEndpoint;
    }

    async writeToDisk(): Promise<TurbopackResult<NapiWrittenEndpoint>> {
      return await withErrorCause(
        () =>
          binding.endpointWriteToDisk(this._nativeEndpoint) as Promise<
            TurbopackResult<NapiWrittenEndpoint>
          >,
      );
    }

    async clientChanged(): Promise<AsyncIterableIterator<TurbopackResult<{}>>> {
      const clientSubscription = subscribe<TurbopackResult>(
        false,
        async (callback) =>
          binding.endpointClientChangedSubscribe(
            await this._nativeEndpoint,
            callback,
          ),
      );
      await clientSubscription.next();
      return clientSubscription;
    }

    async serverChanged(
      includeIssues: boolean,
    ): Promise<AsyncIterableIterator<TurbopackResult<{}>>> {
      const serverSubscription = subscribe<TurbopackResult>(
        false,
        async (callback) =>
          binding.endpointServerChangedSubscribe(
            await this._nativeEndpoint,
            includeIssues,
            callback,
          ),
      );
      await serverSubscription.next();
      return serverSubscription;
    }
  }

  function napiEntrypointsToRawEntrypoints(
    entrypoints: TurbopackResult<{
      apps?: { __napiType: "Endpoint" }[];
      libraries?: { __napiType: "Endpoint" }[];
      appPaths?: NapiWrittenEndpoint[];
      libraryPaths?: NapiWrittenEndpoint[];
    }>,
  ) {
    return {
      apps: (entrypoints.apps || []).map((e) => new EndpointImpl(e)),
      libraries: (entrypoints.libraries || []).map((e) => new EndpointImpl(e)),
      appPaths: entrypoints.appPaths,
      libraryPaths: entrypoints.libraryPaths,
      issues: entrypoints.issues,
    };
  }

  return async function createProject(
    options: Required<ProjectOptions>,
    turboEngineOptions: binding.NapiTurboEngineOptions,
  ) {
    return new ProjectImpl(
      await binding.projectNew(
        await rustifyProjectOptions(options),
        turboEngineOptions || {},
        {
          throwTurbopackInternalError,
        },
      ),
    );
  };
}
