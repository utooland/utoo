/**
 * This file contains runtime types and functions that are shared between all
 * Turbopack *development* ECMAScript runtimes.
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./globals.d.ts" />
/// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-utils.ts" />


// Used in WebWorkers to tell the runtime about the chunk base path
declare var TURBOPACK_WORKER_LOCATION: string;
// Used in WebWorkers to tell the runtime about the current chunk url since it can't be detected via document.currentScript
// Note it's stored in reversed order to use push and pop
declare var TURBOPACK_NEXT_CHUNK_URLS: ChunkUrl[] | undefined;

// Injected by rust code
declare var CHUNK_BASE_PATH: string;
declare var CHUNK_SUFFIX_PATH: string;

interface TurbopackBrowserBaseContext<M> extends TurbopackBaseContext<M> {
  R: ResolvePathFromModule;
}

const browserContextPrototype =
  Context.prototype as TurbopackBrowserBaseContext<unknown>;

// Provided by build
declare function instantiateModule(
  id: ModuleId,
  sourceType: SourceType,
  sourceData: SourceData,
): Module;

type RuntimeParams = {
  otherChunks: ChunkData[];
  runtimeModuleIds: ModuleId[];
};

type ChunkRegistrationChunk =
  | ChunkPath
  | { getAttribute: (name: string) => string | null }
  | undefined;

type ChunkRegistration = [
  chunkPath: ChunkRegistrationChunk,
  ...([RuntimeParams] | CompressedModuleFactories),
];

type ChunkList = {
  script: ChunkRegistrationChunk;
  chunks: ChunkData[];
  source: "entry" | "dynamic";
};

// SourceType and SourceData are provided by shared/runtime/runtime-utils.ts
interface RuntimeBackend {
  registerChunk: (chunkPath: ChunkPath | ChunkScript, params?: RuntimeParams) => void;
  /**
   * Returns the same Promise for the same chunk URL.
   */
  loadChunkCached: (
    sourceType: SourceType,
    chunkUrl: ChunkUrl,
  ) => Promise<void>;
}

const moduleFactories: ModuleFactories = new Map();
contextPrototype.M = moduleFactories;

const availableModules: Map<ModuleId, Promise<any> | true> = new Map();

const availableModuleChunks: Map<ChunkPath, Promise<any> | true> = new Map();

const loadedChunk = Promise.resolve(undefined);
const instrumentedBackendLoadChunks = new WeakMap<
  Promise<any>,
  Promise<any> | typeof loadedChunk
>();
// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkByUrl(
  this: TurbopackBrowserBaseContext<Module>,
  chunkUrl: ChunkUrl,
) {
  return loadChunkByUrlInternal(SourceType.Parent, this.m.id, chunkUrl);
}
browserContextPrototype.L = loadChunkByUrl;

const loadedScripts = new Map<string, Promise<void>>();

/**
 * Load an external script by creating a <script> tag.
 * This is used for script externals that need to be loaded from CDN or other external sources.
 */
function loadScript(
  this: TurbopackBrowserBaseContext<Module>,
  scriptUrl: string,
): Promise<void> {
  // Return cached promise if script is already loading or loaded
  let promise = loadedScripts.get(scriptUrl);
  if (promise) {
    return promise;
  }

  promise = new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = scriptUrl;
    script.onload = () => resolve();
    script.onerror = () =>
      reject(new Error(`Failed to load script: ${scriptUrl}`));
    document.head.appendChild(script);
  });

  loadedScripts.set(scriptUrl, promise);
  return promise;
}
browserContextPrototype.S = loadScript;

// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkByUrlInternal(
  sourceType: SourceType,
  sourceData: SourceData,
  chunkUrl: ChunkUrl,
): Promise<any> {
  const thenable = BACKEND.loadChunkCached(sourceType, chunkUrl);
  let entry = instrumentedBackendLoadChunks.get(thenable);
  if (entry === undefined) {
    const resolve = instrumentedBackendLoadChunks.set.bind(
      instrumentedBackendLoadChunks,
      thenable,
      loadedChunk,
    );
    entry = thenable.then(resolve).catch((error) => {
      let loadReason;
      switch (sourceType) {
        case SourceType.Runtime:
          loadReason = `as a runtime dependency of chunk ${sourceData}`;
          break;
        case SourceType.Parent:
          loadReason = `from module ${sourceData}`;
          break;
        case SourceType.Update:
          loadReason = "from an HMR update";
          break;
        default:
          invariant(
            sourceType,
            (sourceType) => `Unknown source type: ${sourceType}`,
          );
      }
      throw new (Error as any)(
        `Failed to load chunk ${chunkUrl} ${loadReason}${
          error ? `: ${error}` : ""
        }`,
        error
          ? {
              cause: error,
            }
          : undefined,
      );
    });
    instrumentedBackendLoadChunks.set(thenable, entry);
  }

  return entry;
}

// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkPath(
  sourceType: SourceType,
  sourceData: SourceData,
  chunkPath: ChunkPath,
): Promise<void> {
  const url = getChunkRelativeUrl(chunkPath);
  return loadChunkByUrlInternal(sourceType, sourceData, url);
}

/**
 * Returns an absolute url to an asset.
 */
function resolvePathFromModule(
  this: TurbopackBaseContext<Module>,
  moduleId: string,
): string {
  const exported = this.r(moduleId);
  return exported?.default ?? exported;
}
browserContextPrototype.R = resolvePathFromModule;

/**
 * no-op for browser
 * @param modulePath
 */
function resolveAbsolutePath(modulePath?: string): string {
  return `/ROOT/${modulePath ?? ""}`;
}
browserContextPrototype.P = resolveAbsolutePath;

/**
 * Instantiates a runtime module.
 */
function instantiateRuntimeModule(
  moduleId: ModuleId,
  chunkPath: ChunkPath,
): Module {
  return instantiateModule(moduleId, SourceType.Runtime, chunkPath);
}
/**
 * Returns the URL relative to the origin where a chunk can be fetched from.
 */
function getChunkRelativeUrl(chunkPath: ChunkPath | ChunkListPath): ChunkUrl {
  return `${CHUNK_BASE_PATH}${chunkPath
    .split("/")
    .map((p) => encodeURIComponent(p))
    .join("/")}${CHUNK_SUFFIX_PATH}` as ChunkUrl;
}

/**
 * Return the ChunkPath from a ChunkScript.
 */
function getPathFromScript(chunkScript: ChunkPath | ChunkScript): ChunkPath;
function getPathFromScript(
  chunkScript: ChunkListPath | ChunkListScript,
): ChunkListPath;
function getPathFromScript(
  chunkScript: ChunkPath | ChunkListPath | ChunkScript | ChunkListScript,
): ChunkPath | ChunkListPath {
  if (typeof chunkScript === "string") {
    return chunkScript as ChunkPath | ChunkListPath;
  }
  const chunkUrl =
    typeof TURBOPACK_NEXT_CHUNK_URLS !== "undefined"
      ? TURBOPACK_NEXT_CHUNK_URLS.pop()!
      : chunkScript.src!;
  const src = decodeURIComponent(chunkUrl.replace(/[?#].*$/, ""));
  let path = src.startsWith(CHUNK_BASE_PATH)
    ? src.slice(CHUNK_BASE_PATH.length)
    : src;
  if (path.startsWith("/")) {
    path = path.slice(1);
  }
  return path as ChunkPath | ChunkListPath;
}


const regexJsUrl = /\.js(?:\?[^#]*)?(?:#.*)?$/;
/**
 * Checks if a given path/URL ends with .js, optionally followed by ?query or #fragment.
 */
function isJs(chunkUrlOrPath: ChunkUrl | ChunkPath): boolean {
  return regexJsUrl.test(chunkUrlOrPath);
}

/**
 * Determine the chunk to register. Note that this function has side-effects!
 */
function getChunkFromRegistration(
  chunk: ChunkRegistrationChunk,
): ChunkPath | CurrentScript {
  if (typeof chunk === "string") {
    return chunk;
  } else if (!chunk) {
    if (typeof TURBOPACK_NEXT_CHUNK_URLS !== "undefined") {
      return { src: TURBOPACK_NEXT_CHUNK_URLS.pop()! } as CurrentScript;
    } else {
      throw new Error("chunk path empty but not in a worker");
    }
  } else {
    return { src: chunk.getAttribute("src")! } as CurrentScript;
  }
}
