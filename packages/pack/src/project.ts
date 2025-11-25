import { isDeepStrictEqual } from "util";
import { Worker } from "worker_threads";
import type {
  HmrIdentifiers,
  NapiPartialProjectOptions,
  NapiProjectOptions,
  NapiUpdateMessage,
  NapiWrittenEndpoint,
  StackFrame,
} from "./binding";
import * as binding from "./binding";
import {
  ConfigComplete,
  Endpoint,
  Project,
  ProjectOptions,
  RawEntrypoints,
  TurbopackLoaderItem,
  TurbopackRuleConfigItem,
  TurbopackRuleConfigItemOptions,
  TurbopackRuleConfigItemOrShortcut,
  Update,
} from "./types";
import { rustifyEnv } from "./util";

export class TurbopackInternalError extends Error {
  name = "TurbopackInternalError";

  constructor(cause: Error) {
    super(cause.message);
    this.stack = cause.stack;
  }
}

async function withErrorCause<T>(fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (nativeError: any) {
    throw new TurbopackInternalError(nativeError);
  }
}

function ensureLoadersHaveSerializableOptions(
  turbopackRules: Record<string, TurbopackRuleConfigItemOrShortcut>,
) {
  for (const [glob, rule] of Object.entries(turbopackRules)) {
    if (Array.isArray(rule)) {
      checkLoaderItems(rule, glob);
    } else {
      checkConfigItem(rule, glob);
    }
  }

  function checkConfigItem(rule: TurbopackRuleConfigItem, glob: string) {
    if (!rule) return;
    if ("loaders" in rule) {
      checkLoaderItems((rule as TurbopackRuleConfigItemOptions).loaders, glob);
    } else {
      for (const key in rule) {
        const inner = rule[key];
        if (typeof inner === "object" && inner) {
          checkConfigItem(inner, glob);
        }
      }
    }
  }

  function checkLoaderItems(loaderItems: TurbopackLoaderItem[], glob: string) {
    for (const loaderItem of loaderItems) {
      if (
        typeof loaderItem !== "string" &&
        !isDeepStrictEqual(loaderItem, JSON.parse(JSON.stringify(loaderItem)))
      ) {
        throw new Error(
          `loader ${loaderItem.loader} for match "${glob}" does not have serializable options. Ensure that options passed are plain JavaScript objects and values.`,
        );
      }
    }
  }
}

async function serializeConfig(config: ConfigComplete): Promise<string> {
  let configSerializable = { ...config };

  if (configSerializable.module?.rules) {
    ensureLoadersHaveSerializableOptions(configSerializable.module.rules);
  }

  if (configSerializable.optimization) {
    configSerializable.optimization.modularizeImports =
      configSerializable.optimization &&
      configSerializable.optimization?.modularizeImports
        ? Object.fromEntries(
            Object.entries<any>(
              configSerializable.optimization.modularizeImports,
            ).map(([mod, config]) => [
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
          )
        : undefined;
  }

  return JSON.stringify(configSerializable, null, 2);
}

async function rustifyPartialProjectOptions(
  options: Partial<ProjectOptions>,
): Promise<NapiPartialProjectOptions> {
  return {
    ...options,
    config: options.config && (await serializeConfig(options.config)),
    processEnv: options.processEnv && rustifyEnv(options.processEnv),
  };
}

type NativeFunction<T> = (
  callback: (err: Error, value: T) => void,
) => Promise<{ __napiType: "RootTask" }>;

async function rustifyProjectOptions(
  options: Required<ProjectOptions>,
): Promise<NapiProjectOptions> {
  return {
    ...options,
    config: await serializeConfig(options.config),
    processEnv: rustifyEnv(options.processEnv ?? {}),
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
          throw new TurbopackInternalError(e);
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

    #poolCreated: Record<string, Array<Worker>> = {};

    #poolScheduler?: ReturnType<typeof setInterval>;

    constructor(nativeProject: { __napiType: "Project" }) {
      this._nativeProject = nativeProject;
      if (binding.recvPoolCreation) {
        this.#poolScheduler = setInterval(() => {
          let poolOptions = binding.recvPoolCreation();
          if (poolOptions) {
            const { filename, concurrency } = poolOptions;
            if (!this.#poolCreated[filename]) {
              const workers = [];
              for (let i = 0; i < concurrency; i++) {
                const worker = new Worker(filename, {
                  workerData: {
                    poolId: filename,
                    bindingPath: require.resolve("./binding.js"),
                  },
                });
                worker.unref();
                workers.push(worker);
              }
              this.#poolCreated[filename] = workers;
            }
          }
        }, 0);
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

    hmrEvents(identifier: string) {
      return subscribe<TurbopackResult<Update>>(true, async (callback) =>
        binding.projectHmrEvents(this._nativeProject, identifier, callback),
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
      this.#poolScheduler && clearInterval(this.#poolScheduler);
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
    }>,
  ) {
    return {
      apps: (entrypoints.apps || []).map((e) => new EndpointImpl(e)),
      libraries: (entrypoints.libraries || []).map((e) => new EndpointImpl(e)),
      issues: entrypoints.issues,
      diagnostics: entrypoints.diagnostics,
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
      ),
    );
  };
}
