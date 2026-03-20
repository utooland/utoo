/**
 * This file contains the runtime code specific to the Turbopack
 * ECMAScript Node.js runtime for library builds.
 *
 * It will be appended to the base runtime code in place of
 * runtime-backend-dom.ts when the target platform is Node.js.
 *
 * Since library builds produce a single, self-contained chunk,
 * no dynamic chunk loading is needed. The BACKEND simply registers
 * modules and instantiates runtime entries.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./runtime-base.ts" />

(() => {
  BACKEND = {
    registerChunk(chunk, params) {
      const chunkPath = typeof chunk === "string"
        ? chunk
        : chunk.src! as unknown as ChunkPath;

      if (params == null) {
        return;
      }

      if (params.runtimeModuleIds.length > 0) {
        for (const moduleId of params.runtimeModuleIds) {
          getOrInstantiateRuntimeModule(chunkPath, moduleId);
        }
      }
    },

    /**
     * In a single-chunk Node.js library build, all modules are already
     * bundled into the same file. This function should never be called.
     */
    loadChunkCached(
      _sourceType: SourceType,
      _chunkUrl: ChunkUrl,
    ) {
      return Promise.resolve();
    },
  };
})();

// Node.js-specific: require.resolve is not available in browser environments
(externalRequire as any).resolve = (
  id: string,
  options?: {
    paths?: string[];
  },
) => {
  return require.resolve(id, options);
};
