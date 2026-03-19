/**
 * This file contains the runtime code specific to the Turbopack
 * ECMAScript DOM runtime for library builds.
 *
 * It will be appended to the base runtime code in place of
 * runtime-backend-node.ts when the target platform is browser/web.
 *
 * Since library builds produce a single, self-contained chunk,
 * no dynamic chunk loading is needed. The BACKEND simply registers
 * modules and instantiates runtime entries.
 *
 * The only DOM-specific addition is `loadScript` for script externals
 * that need to be loaded from CDN or other external sources.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./runtime-base.ts" />

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

(() => {
  BACKEND = {
    registerChunk(chunk, params) {
      if (params == null) {
        return;
      }

      if (params.runtimeModuleIds.length > 0) {
        for (const moduleId of params.runtimeModuleIds) {
          getOrInstantiateRuntimeModule(chunk as ChunkPath, moduleId);
        }
      }
    },

    /**
     * In a single-chunk browser library build, all modules are already
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
