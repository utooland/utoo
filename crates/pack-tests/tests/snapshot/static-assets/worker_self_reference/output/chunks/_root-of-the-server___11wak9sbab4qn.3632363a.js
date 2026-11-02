(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/static-assets/worker_self_reference/input/index.js [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

var __TURBOPACK__import$2e$meta__ = {
    get url () {
        return __turbopack_context__.F("input/index.js");
    },
    env: {
        DEV: false,
        PROD: true,
        MODE: "production",
        BASE_URL: "/",
        SSR: false
    }
};
globalThis.worker = __turbopack_context__.r("[project]/static-assets/worker_self_reference/input/worker.js [client] (ecmascript, worker loader)")(Worker, {
    type: "module"
});
}),
"[project]/static-assets/worker_self_reference/input/worker.js (static in ecmascript)", ((__turbopack_context__) => {

__turbopack_context__.q("/worker.83baada5.js");}),
"[project]/static-assets/worker_self_reference/input/worker.js [client] (ecmascript, worker loader)", ((__turbopack_context__) => {

__turbopack_context__.v(__turbopack_context__.r("[turbopack-ecmascript]/worker/browser/createWorker.ts [client] (ecmascript)")["f"]("chunks/turbopack-worker-_client-fs___0uy0mninb8pht.735ecc6d.js", "[project]/static-assets/worker_self_reference/input/worker.js [client] (ecmascript, worker loader)", ["[project]/static-assets/worker_self_reference/input/worker.js [client] (ecmascript, worker loader)"]));
}),
"[turbopack-ecmascript]/worker/browser/createWorker.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

// Embedded worker-runtime helper. This file is bundled as a regular module and
// `__turbopack_require__`d by the generated web-worker loader code.
//
// The chunk-URL builder, the chunk base path and the asset suffix are read from
// the shared `__turbopack_chunk_relative_url__` / `__turbopack_chunk_base_path__`
// / `__turbopack_chunk_asset_suffix__` runtime primitives. The worker base-path
// override and forwarded-global names are baked into this module at build time by
// `turbopack-ecmascript` replacing the `_TURBOPACK_WORKER_BASE_PATH_` /
// `_TURBOPACK_WORKER_FORWARDED_GLOBALS_` free variables, and the forwarded-global
// values are read from `globalThis`.
/**
 * Creates a web worker by instantiating the given WorkerConstructor with the
 * appropriate URL and options.
 *
 * The entrypoint is a pre-compiled worker runtime file. The params configure
 * which module chunks to load and which module to run as the entry point.
 *
 * The params are a JSON array of the following structure:
 * `[TURBOPACK_NEXT_CHUNK_URLS, ASSET_SUFFIX, WORKER_CHUNK_BASE_PATH, ...workerForwardedGlobals values, workerRegistry?]`
 *
 * @param WorkerConstructor The Worker or SharedWorker constructor
 * @param entrypoint path to the worker entrypoint chunk
 * @param moduleChunks list of module chunk paths to load
 * @param workerIds reachable nested worker loaders whose configuration must be forwarded
 * @param workerOptions options to pass to the Worker constructor (optional)
 */ function createWorker(WorkerConstructor, entrypoint, moduleChunks, workerIds, workerOptions) {
    const isSharedWorker = WorkerConstructor.name === 'SharedWorker';
    // `WORKER_BASE_PATH` overrides `CHUNK_BASE_PATH` for the entrypoint and the
    // module chunks loaded inside the worker, keeping them same-origin to each
    // other when `CHUNK_BASE_PATH` (= `assetPrefix`) is a cross-origin CDN.
    // `null` falls back; an empty string is treated as a literal empty prefix.
    const workerBasePath = null ?? /*TURBOPACK member replacement*/ __turbopack_context__.b;
    const chunkUrls = moduleChunks.map((chunk)=>/*TURBOPACK member replacement*/ __turbopack_context__.h(typeof chunk === 'string' ? chunk : chunk.path, workerBasePath)).reverse();
    const params = [
        chunkUrls,
        /*TURBOPACK member replacement*/ __turbopack_context__.X,
        workerBasePath
    ];
    const globals = [];
    for(let i = 0; i < globals.length; i++){
        params.push(globalThis[globals[i]]);
    }
    // Forward only reachable nested workers. Unrelated worker changes must not
    // change this URL and split a SharedWorker's shared instance.
    if (workerIds.length > 0) {
        params.push(workerIds.map((id)=>[
                id,
                getWorkerChunks(id)
            ]));
    }
    const url = new URL(/*TURBOPACK member replacement*/ __turbopack_context__.h(entrypoint, workerBasePath), location.origin);
    const paramsJson = JSON.stringify(params);
    if (isSharedWorker) {
        url.searchParams.set('params', paramsJson);
    } else {
        url.hash = '#params=' + encodeURIComponent(paramsJson);
    }
    // Remove type: "module" from options since our worker entrypoint is not a module
    const options = workerOptions ? {
        ...workerOptions,
        type: undefined
    } : undefined;
    return new WorkerConstructor(url, options);
}
function getWorkerChunks(workerId) {
    const chunks = /*TURBOPACK member replacement*/ __turbopack_context__.W.get(workerId);
    if (!chunks) {
        throw new Error(`Missing chunk configuration for worker ${workerId}`);
    }
    return chunks;
}
function generateCreateWorker(entrypoint, workerIdOrChunks, workerIds = []) {
    return (WorkerConstructor, workerOptions)=>{
        const moduleChunks = Array.isArray(workerIdOrChunks) ? workerIdOrChunks : getWorkerChunks(workerIdOrChunks);
        return createWorker(WorkerConstructor, entrypoint, moduleChunks, workerIds, workerOptions);
    };
}
__turbopack_context__.s([
    "f",
    0,
    generateCreateWorker
]);
}),
]);