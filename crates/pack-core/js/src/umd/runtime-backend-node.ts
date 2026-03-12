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
/// <reference path="./runtime-types.d.ts" />

let BACKEND: RuntimeBackend;

(() => {
  BACKEND = {
    registerChunk(chunkPath, params) {
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
      _sourceData: SourceData,
      _chunkUrl: ChunkUrl,
    ) {
      return Promise.resolve();
    },
  };
})();
