/**
 * This file contains the runtime code specific to the Turbopack
 * ECMAScript Node.js runtime for library builds.
 *
 * It will be appended to the base runtime code in place of
 * runtime-backend-dom.ts when the target platform is Node.js.
 *
 * Server library entry chunks can reference shared chunks. Those chunks are
 * CommonJS modules exporting compressed module factories, so the backend can
 * load them synchronously before instantiating runtime entries.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./runtime-base.ts" />

async function externalImport(id: DependencySpecifier) {
  let raw;
  try {
    raw = await import(id);
  } catch (err) {
    // TODO(alexkirsz) This can happen when a client-side module tries to load
    // an external module we don't provide a shim for (e.g. querystring, url).
    // For now, we fail semi-silently, but in the future this should be a
    // compilation error.
    throw new Error(`Failed to load external module ${id}: ${err}`);
  }

  if (raw && raw.__esModule && raw.default && "default" in raw.default) {
    return interopEsm(raw.default, createNS(raw), true);
  }

  return raw;
}
contextPrototype.y = externalImport;

/**
 * Exports a URL value. No suffix is added in Node.js runtime.
 */
function exportUrl(
  this: TurbopackBaseContext<Module>,
  url: string,
  id: ModuleId | undefined,
) {
  exportValue.call(this, url, id);
}
contextPrototype.q = exportUrl;

(() => {
  BACKEND = {
    registerChunk(chunk, params) {
      const chunkPath =
        typeof chunk === "string"
          ? chunk
          : (chunk.src! as unknown as ChunkPath);

      if (params == null) {
        return;
      }

      const otherChunks = (
        params as RuntimeParams & { otherChunks: ChunkData[] }
      ).otherChunks;
      const nodePath = require("path");
      for (const otherChunk of otherChunks) {
        const otherChunkPath = getChunkPath(otherChunk);
        if (!/\.(?:c|m)?js(?:\?|$)/.test(otherChunkPath)) {
          continue;
        }
        const relativeChunkPath = nodePath.relative(
          nodePath.dirname(chunkPath),
          otherChunkPath,
        );
        const chunkModules: CompressedModuleFactories = require(
          nodePath.resolve(__dirname, relativeChunkPath),
        );
        installCompressedModuleFactories(chunkModules, 0, moduleFactories);
      }

      if (params.runtimeModuleIds.length > 0) {
        for (const moduleId of params.runtimeModuleIds) {
          getOrInstantiateRuntimeModule(chunkPath, moduleId);
        }
      }
    },
  };
})();
