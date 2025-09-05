/**
 * This file contains the runtime code specific to the Turbopack development
 * ECMAScript DOM runtime.
 *
 * It will be appended to the base development runtime code.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./runtime-base.ts" />
/// <reference path="./runtime-types.d.ts" />

type ChunkResolver = {
  resolved: boolean;
  loadingStarted: boolean;
  resolve: () => void;
  reject: (error?: Error) => void;
  promise: Promise<any>;
};

let BACKEND: RuntimeBackend;

/**
 * Maps chunk paths to the corresponding resolver.
 */
const chunkResolvers: Map<ChunkUrl, ChunkResolver> = new Map();

(() => {
  BACKEND = {
    registerChunk(chunkPath, params) {
      const chunkUrl = getChunkRelativeUrl(chunkPath);

      const resolver = getOrCreateResolver(chunkUrl);
      resolver.resolve();

      if (params == null) {
        return;
      }

      for (const otherChunkData of params.otherChunks) {
        const otherChunkPath = getChunkPath(otherChunkData);
        const otherChunkUrl = getChunkRelativeUrl(otherChunkPath);

        // Chunk might have started loading, so we want to avoid triggering another load.
        getOrCreateResolver(otherChunkUrl);
      }

      if (params.runtimeModuleIds.length > 0) {
        for (const moduleId of params.runtimeModuleIds) {
          getOrInstantiateRuntimeModule(chunkPath, moduleId);
        }
      }
    },

    /**
     * Loads the given chunk, and returns a promise that resolves once the chunk
     * has been loaded.
     */
    loadChunkCached(
      sourceType: SourceType,
      sourceData: SourceData,
      chunkUrl: ChunkUrl,
    ) {
      return doLoadChunk(sourceType, sourceData, chunkUrl);
    },
  };
  function getOrCreateResolver(chunkUrl: ChunkUrl): ChunkResolver {
    let resolver = chunkResolvers.get(chunkUrl);
    if (!resolver) {
      let resolve: () => void;
      let reject: (error?: Error) => void;
      const promise = new Promise<void>((innerResolve, innerReject) => {
        resolve = innerResolve;
        reject = innerReject;
      });
      resolver = {
        resolved: false,
        loadingStarted: false,
        promise,
        resolve: () => {
          resolver!.resolved = true;
          resolve();
        },
        reject: reject!,
      };
      chunkResolvers.set(chunkUrl, resolver);
    }
    return resolver;
  }

  /**
   * Loads the given chunk, and returns a promise that resolves once the chunk
   * has been loaded.
   */
  function doLoadChunk(
    sourceType: SourceType,
    _sourceData: SourceData,
    chunkUrl: ChunkUrl,
  ) {
    const resolver = getOrCreateResolver(chunkUrl);
    if (resolver.loadingStarted) {
      return resolver.promise;
    }

    if (sourceType === SourceType.Runtime) {
      // We don't need to load chunks references from runtime code, as they're already
      // present in the DOM.
      resolver.loadingStarted = true;

      // We need to wait for JS chunks to register themselves within `registerChunk`
      // before we can start instantiating runtime modules, hence the absence of
      // `resolver.resolve()` in this branch.

      return resolver.promise;
    }

    if (typeof importScripts === "function") {
      // We're in a web worker
      if (isJs(chunkUrl)) {
        self.TURBOPACK_NEXT_CHUNK_URLS!.push(chunkUrl);
        importScripts(TURBOPACK_WORKER_LOCATION + chunkUrl);
      } else {
        throw new Error(
          `can't infer type of chunk from URL ${chunkUrl} in worker`,
        );
      }
    } else {
      // TODO(PACK-2140): remove this once all filenames are guaranteed to be escaped.
      const decodedChunkUrl = decodeURI(chunkUrl);

      if (isJs(chunkUrl)) {
        const previousScripts = document.querySelectorAll(
          `script[src="${chunkUrl}"],script[src^="${chunkUrl}?"],script[src="${decodedChunkUrl}"],script[src^="${decodedChunkUrl}?"]`,
        );
        if (previousScripts.length > 0) {
          // There is this edge where the script already failed loading, but we
          // can't detect that. The Promise will never resolve in this case.
          for (const script of Array.from(previousScripts)) {
            script.addEventListener("error", () => {
              resolver.reject();
            });
          }
        } else {
          const script = document.createElement("script");
          script.src = chunkUrl;
          // We'll only mark the chunk as loaded once the script has been executed,
          // which happens in `registerChunk`. Hence the absence of `resolve()` in
          // this branch.
          script.onerror = () => {
            resolver.reject();
          };
          // Append to the `head` for webpack compatibility.
          document.head.appendChild(script);
        }
      } else {
        throw new Error(`can't infer type of chunk from URL ${chunkUrl}`);
      }
    }

    resolver.loadingStarted = true;
    return resolver.promise;
  }
})();
