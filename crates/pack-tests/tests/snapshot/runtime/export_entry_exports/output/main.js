(globalThis["utooChunk_export_entry_exports"] || (globalThis["utooChunk_export_entry_exports"] = [])).push([
    typeof document === "object" ? document.currentScript : undefined,
    {"otherChunks":["input_index_ts_6bcb050d.js"],"runtimeModuleIds":["[project]/runtime/export_entry_exports/input/index.ts [client] (ecmascript)"]}
]);
(() => {
(function(root, factory) {
    if (typeof exports === 'object' && typeof module === 'object')
        module.exports = factory();
    else if (typeof exports === 'object')
        exports["export-entry-exports"] = factory();
    else
        root["export-entry-exports"] = factory();
}(typeof self !== 'undefined' ? self : this, function() {

const __chunk__ = (() => {
if (!Array.isArray(globalThis["utooChunk_export_entry_exports"])) {
    return;
}

let __entryExports__ = undefined;

var CHUNK_BASE_PATH = "/";
var WORKER_BASE_PATH = null;
var RELATIVE_ROOT_PATH = "/ROOT";
var RUNTIME_PUBLIC_PATH = "/";
var ASSET_SUFFIX = "";
var CROSS_ORIGIN = null;
var CHUNK_LOAD_RETRY_MAX_ATTEMPTS = 1;
var CHUNK_LOAD_RETRY_BASE_DELAY_MS = 200;
var CHUNK_LOAD_RETRY_MAX_JITTER_MS = 400;
var WORKER_FORWARDED_GLOBALS = [];
/**
 * This file contains runtime types and functions that are shared between all
 * TurboPack ECMAScript runtimes.
 *
 * It will be prepended to the runtime code of each runtime.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./runtime-types.d.ts" />
/// <reference path="./async-module.ts" />
/**
 * Describes why a module was instantiated.
 * Shared between browser and Node.js runtimes.
 */ var SourceType = /*#__PURE__*/ function(SourceType) {
    /**
   * The module was instantiated because it was included in an evaluated chunk's
   * runtime.
   * SourceData is a ChunkPath.
   */ SourceType[SourceType["Runtime"] = 0] = "Runtime";
    /**
   * The module was instantiated because a parent module imported it.
   * SourceData is a ModuleId.
   */ SourceType[SourceType["Parent"] = 1] = "Parent";
    /**
   * The module was instantiated because it was included in a chunk's hot module
   * update.
   * SourceData is an array of ModuleIds or undefined.
   */ SourceType[SourceType["Update"] = 2] = "Update";
    return SourceType;
}(SourceType || {});
/**
 * Flag indicating which module object type to create when a module is merged. Set to `true`
 * by each runtime that uses ModuleWithDirection (browser dev-base.ts, nodejs dev-base.ts,
 * nodejs build-base.ts). Browser production (build-base.ts) leaves it as `false` since it
 * uses plain Module objects.
 */ let createModuleWithDirectionFlag = false;
const REEXPORTED_OBJECTS = new WeakMap();
/**
 * Constructs the `__turbopack_context__` object for a module.
 */ function Context(module, exports) {
    this.m = module;
    // We need to store this here instead of accessing it from the module object to:
    // 1. Make it available to factories directly, since we rewrite `this` to
    //    `__turbopack_context__.e` in CJS modules.
    // 2. Support async modules which rewrite `module.exports` to a promise, so we
    //    can still access the original exports object from functions like
    //    `esmExport`
    // Ideally we could find a new approach for async modules and drop this property altogether.
    this.e = exports;
}
const contextPrototype = Context.prototype;
const hasOwnProperty = Object.prototype.hasOwnProperty;
const toStringTag = typeof Symbol !== 'undefined' && Symbol.toStringTag;
function defineProp(obj, name, options) {
    if (!hasOwnProperty.call(obj, name)) Object.defineProperty(obj, name, options);
}
function getOverwrittenModule(moduleCache, id) {
    let module = moduleCache[id];
    if (!module) {
        if (createModuleWithDirectionFlag) {
            // set in development modes for hmr support
            module = createModuleWithDirection(id);
        } else {
            module = createModuleObject(id);
        }
        moduleCache[id] = module;
    }
    return module;
}
/**
 * Creates the module object. Only done here to ensure all module objects have the same shape.
 */ function createModuleObject(id) {
    return {
        exports: {},
        error: undefined,
        id,
        namespaceObject: undefined
    };
}
function createModuleWithDirection(id) {
    return {
        exports: {},
        error: undefined,
        id,
        namespaceObject: undefined,
        parents: [],
        children: []
    };
}
const BindingTag_Value = 0;
/**
 * Adds the getters to the exports object.
 */ function esm(exports, bindings) {
    defineProp(exports, '__esModule', {
        value: true
    });
    if (toStringTag) defineProp(exports, toStringTag, {
        value: 'Module'
    });
    let i = 0;
    while(i < bindings.length){
        const propName = bindings[i++];
        const tagOrFunction = bindings[i++];
        if (typeof tagOrFunction === 'number') {
            if (tagOrFunction === BindingTag_Value) {
                defineProp(exports, propName, {
                    value: bindings[i++],
                    enumerable: true,
                    writable: false
                });
            } else {
                throw new Error(`unexpected tag: ${tagOrFunction}`);
            }
        } else {
            const getterFn = tagOrFunction;
            if (typeof bindings[i] === 'function') {
                const setterFn = bindings[i++];
                defineProp(exports, propName, {
                    get: getterFn,
                    set: setterFn,
                    enumerable: true
                });
            } else {
                defineProp(exports, propName, {
                    get: getterFn,
                    enumerable: true
                });
            }
        }
    }
    Object.seal(exports);
}
/**
 * Makes the module an ESM with exports
 */ function esmExport(bindings, id) {
    let module;
    let exports;
    if (id != null) {
        module = getOverwrittenModule(this.c, id);
        exports = module.exports;
    } else {
        module = this.m;
        exports = this.e;
    }
    module.namespaceObject = exports;
    esm(exports, bindings);
}
contextPrototype.s = esmExport;
function ensureDynamicExports(module, exports) {
    let reexportedObjects = REEXPORTED_OBJECTS.get(module);
    if (!reexportedObjects) {
        REEXPORTED_OBJECTS.set(module, reexportedObjects = []);
        module.exports = module.namespaceObject = new Proxy(exports, {
            get (target, prop) {
                if (hasOwnProperty.call(target, prop) || prop === 'default' || prop === '__esModule') {
                    return Reflect.get(target, prop);
                }
                for (const obj of reexportedObjects){
                    const value = Reflect.get(obj, prop);
                    if (value !== undefined) return value;
                }
                return undefined;
            },
            ownKeys (target) {
                const keys = Reflect.ownKeys(target);
                for (const obj of reexportedObjects){
                    for (const key of Reflect.ownKeys(obj)){
                        if (key !== 'default' && !keys.includes(key)) keys.push(key);
                    }
                }
                return keys;
            }
        });
    }
    return reexportedObjects;
}
/**
 * Dynamically exports properties from an object
 */ function dynamicExport(object, id) {
    let module;
    let exports;
    if (id != null) {
        module = getOverwrittenModule(this.c, id);
        exports = module.exports;
    } else {
        module = this.m;
        exports = this.e;
    }
    const reexportedObjects = ensureDynamicExports(module, exports);
    if (typeof object === 'object' && object !== null) {
        reexportedObjects.push(object);
    }
}
contextPrototype.j = dynamicExport;
function exportValue(value, id) {
    let module;
    if (id != null) {
        module = getOverwrittenModule(this.c, id);
    } else {
        module = this.m;
    }
    module.exports = value;
}
contextPrototype.v = exportValue;
function exportNamespace(namespace, id) {
    let module;
    if (id != null) {
        module = getOverwrittenModule(this.c, id);
    } else {
        module = this.m;
    }
    module.exports = module.namespaceObject = namespace;
}
contextPrototype.n = exportNamespace;
function createGetter(obj, key) {
    return ()=>obj[key];
}
/**
 * @returns prototype of the object
 */ const getProto = Object.getPrototypeOf ? (obj)=>Object.getPrototypeOf(obj) : (obj)=>obj.__proto__;
/** Prototypes that are not expanded for exports */ const LEAF_PROTOTYPES = [
    null,
    getProto({}),
    getProto([]),
    getProto(getProto)
];
/**
 * @param raw
 * @param ns
 * @param allowExportDefault
 *   * `false`: will have the raw module as default export
 *   * `true`: will have the default property as default export
 */ function interopEsm(raw, ns, allowExportDefault) {
    const bindings = [];
    let defaultLocation = -1;
    for(let current = raw; (typeof current === 'object' || typeof current === 'function') && !LEAF_PROTOTYPES.includes(current); current = getProto(current)){
        for (const key of Object.getOwnPropertyNames(current)){
            bindings.push(key, createGetter(raw, key));
            if (defaultLocation === -1 && key === 'default') {
                defaultLocation = bindings.length - 1;
            }
        }
    }
    // this is not really correct
    // we should set the `default` getter if the imported module is a `.cjs file`
    if (!(allowExportDefault && defaultLocation >= 0)) {
        // Replace the binding with one for the namespace itself in order to preserve iteration order.
        if (defaultLocation >= 0) {
            // Replace the getter with the value
            bindings.splice(defaultLocation, 1, BindingTag_Value, raw);
        } else {
            bindings.push('default', BindingTag_Value, raw);
        }
    }
    esm(ns, bindings);
    return ns;
}
function createNS(raw) {
    if (typeof raw === 'function') {
        return function(...args) {
            return raw.apply(this, args);
        };
    } else {
        return Object.create(null);
    }
}
function esmImport(id) {
    const module = getOrInstantiateModuleFromParent(id, this.m);
    // any ES module has to have `module.namespaceObject` defined.
    if (module.namespaceObject) return module.namespaceObject;
    // only ESM can be an async module, so we don't need to worry about exports being a promise here.
    const raw = module.exports;
    return module.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
}
contextPrototype.i = esmImport;
function asyncLoader(moduleId) {
    const loader = this.r(moduleId);
    return loader(esmImport.bind(this));
}
contextPrototype.A = asyncLoader;
// Add a simple runtime require so that environments without one can still pass
// `typeof require` CommonJS checks so that exports are correctly registered.
const runtimeRequire = // @ts-ignore
typeof require === 'function' ? require : function require1() {
    throw new Error('Unexpected use of runtime require');
};
contextPrototype.t = runtimeRequire;
function commonJsRequire(id) {
    return getOrInstantiateModuleFromParent(id, this.m).exports;
}
contextPrototype.r = commonJsRequire;
/**
 * Remove fragments and query parameters since they are never part of the context map keys
 *
 * This matches how we parse patterns at resolving time.  Arguably we should only do this for
 * strings passed to `import` but the resolve does it for `import` and `require` and so we do
 * here as well.
 */ function parseRequest(request) {
    // Per the URI spec fragments can contain `?` characters, so we should trim it off first
    // https://datatracker.ietf.org/doc/html/rfc3986#section-3.5
    const hashIndex = request.indexOf('#');
    if (hashIndex !== -1) {
        request = request.substring(0, hashIndex);
    }
    const queryIndex = request.indexOf('?');
    if (queryIndex !== -1) {
        request = request.substring(0, queryIndex);
    }
    return request;
}
/**
 * `require.context` and require/import expression runtime.
 */ function moduleContext(map) {
    function moduleContext(id) {
        id = parseRequest(id);
        if (hasOwnProperty.call(map, id)) {
            return map[id].module();
        }
        const e = new Error(`Cannot find module '${id}'`);
        e.code = 'MODULE_NOT_FOUND';
        throw e;
    }
    moduleContext.keys = ()=>{
        return Object.keys(map);
    };
    moduleContext.resolve = (id)=>{
        id = parseRequest(id);
        if (hasOwnProperty.call(map, id)) {
            return map[id].id();
        }
        const e = new Error(`Cannot find module '${id}'`);
        e.code = 'MODULE_NOT_FOUND';
        throw e;
    };
    moduleContext.import = async (id)=>{
        return await moduleContext(id);
    };
    return moduleContext;
}
contextPrototype.f = moduleContext;
/**
 * Returns the path of a chunk defined by its data.
 */ function getChunkPath(chunkData) {
    return typeof chunkData === 'string' ? chunkData : chunkData.path;
}
// Load the CompressedmoduleFactories of a chunk into the `moduleFactories` Map.
// The CompressedModuleFactories format is
// - 1 or more module ids
// - a module factory function
// So walking this is a little complex but the flat structure is also fast to
// traverse, we can use `typeof` operators to distinguish the two cases.
function installCompressedModuleFactories(chunkModules, offset, moduleFactories, newModuleId) {
    let i = offset;
    while(i < chunkModules.length){
        let end = i + 1;
        // Find our factory function
        while(end < chunkModules.length && typeof chunkModules[end] !== 'function'){
            end++;
        }
        if (end === chunkModules.length) {
            throw new Error('malformed chunk format, expected a factory function');
        }
        // Install the factory for each module ID that doesn't already have one.
        // When some IDs in this group already have a factory, reuse that existing
        // group factory for the missing IDs to keep all IDs in the group consistent.
        // Otherwise, install the factory from this chunk.
        const moduleFactoryFn = chunkModules[end];
        let existingGroupFactory = undefined;
        for(let j = i; j < end; j++){
            const id = chunkModules[j];
            const existingFactory = moduleFactories.get(id);
            if (existingFactory) {
                existingGroupFactory = existingFactory;
                break;
            }
        }
        const factoryToInstall = existingGroupFactory ?? moduleFactoryFn;
        let didInstallFactory = false;
        for(let j = i; j < end; j++){
            const id = chunkModules[j];
            if (!moduleFactories.has(id)) {
                if (!didInstallFactory) {
                    if (factoryToInstall === moduleFactoryFn) {
                        applyModuleFactoryName(moduleFactoryFn);
                    }
                    didInstallFactory = true;
                }
                moduleFactories.set(id, factoryToInstall);
                newModuleId?.(id);
            }
        }
        i = end + 1; // end is pointing at the last factory advance to the next id or the end of the array.
    }
}
/**
 * A pseudo "fake" URL object to resolve to its relative path.
 *
 * When UrlRewriteBehavior is set to relative, calls to the `new URL()` will construct url without base using this
 * runtime function to generate context-agnostic urls between different rendering context, i.e ssr / client to avoid
 * hydration mismatch.
 *
 * This is based on webpack's existing implementation:
 * https://github.com/webpack/webpack/blob/87660921808566ef3b8796f8df61bd79fc026108/lib/runtime/RelativeUrlRuntimeModule.js
 */ const relativeURL = function relativeURL(inputUrl) {
    const realUrl = new URL(inputUrl, 'x:/');
    const values = {};
    for(const key in realUrl)values[key] = realUrl[key];
    values.href = inputUrl;
    values.pathname = inputUrl.replace(/[?#].*/, '');
    values.origin = values.protocol = '';
    values.toString = values.toJSON = (..._args)=>inputUrl;
    for(const key in values)Object.defineProperty(this, key, {
        enumerable: true,
        configurable: true,
        value: values[key]
    });
};
relativeURL.prototype = URL.prototype;
contextPrototype.U = relativeURL;
/**
 * Utility function to ensure all variants of an enum are handled.
 */ function invariant(never, computeMessage) {
    throw new Error(`Invariant: ${computeMessage(never)}`);
}
/**
 * Constructs an error message for when a module factory is not available.
 */ function factoryNotAvailableMessage(moduleId, sourceType, sourceData) {
    let instantiationReason;
    switch(sourceType){
        case 0:
            instantiationReason = `as a runtime entry of chunk ${sourceData}`;
            break;
        case 1:
            instantiationReason = `because it was required from module ${sourceData}`;
            break;
        case 2:
            instantiationReason = 'because of an HMR update';
            break;
        default:
            invariant(sourceType, (sourceType)=>`Unknown source type: ${sourceType}`);
    }
    return `Module ${moduleId} was instantiated ${instantiationReason}, but the module factory is not available.`;
}
/**
 * A stub function to make `require` available but non-functional in ESM.
 */ function requireStub(_moduleId) {
    throw new Error('dynamic usage of require is not supported');
}
contextPrototype.z = requireStub;
// Make `globalThis` available to the module in a way that cannot be shadowed by a local variable.
contextPrototype.g = globalThis;
let cachedAutomaticPublicPath;
function getAutomaticPublicPath() {
    if (cachedAutomaticPublicPath !== undefined) {
        return cachedAutomaticPublicPath;
    }
    let scriptUrl;
    if (typeof document === 'object') {
        const currentScript = document.currentScript;
        scriptUrl = currentScript?.src;
        if (!scriptUrl) {
            const scripts = document.getElementsByTagName('script');
            const script = scripts[scripts.length - 1];
            scriptUrl = script?.src;
        }
    }
    if (!scriptUrl && typeof globalThis.importScripts === 'function' && globalThis.location) {
        scriptUrl = String(globalThis.location);
    }
    cachedAutomaticPublicPath = scriptUrl ? scriptUrl.replace(/^blob:/, '').replace(/#.*$/, '').replace(/\?.*$/, '').replace(/\/[^/]*$/, '/') : '';
    return cachedAutomaticPublicPath;
}
/**
 * Gets the public path for runtime assets.
 * Checks globalThis.publicPath and falls back to empty string.
 */ function getPublicPath(mode) {
    if (mode === 'auto') {
        return getAutomaticPublicPath();
    }
    if (typeof globalThis !== 'undefined' && typeof globalThis.publicPath === 'string') {
        const publicPath = globalThis.publicPath;
        return publicPath.endsWith('/') ? publicPath : `${publicPath}/`;
    }
    return '';
}
contextPrototype.p = getPublicPath;
function applyModuleFactoryName(factory) {
    // Give the module factory a nice name to improve stack traces.
    Object.defineProperty(factory, 'name', {
        value: 'module evaluation'
    });
}
/**
 * This file contains runtime types and functions that are shared between all
 * Turbopack *browser* ECMAScript runtimes.
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="../base/globals.d.ts" />
/// <reference path="../../../shared/runtime/runtime-utils.ts" />
// Used in WebWorkers to tell the runtime about the chunk suffix
// Support runtime public path modes.
function getRuntimeChunkBasePath(basePath = CHUNK_BASE_PATH) {
    if (basePath === '__RUNTIME_PUBLIC_PATH__') {
        return contextPrototype.p();
    }
    if (basePath === '__AUTO_PUBLIC_PATH__') {
        return contextPrototype.p('auto');
    }
    return basePath;
}
const browserContextPrototype = Context.prototype;
const moduleFactories = new Map();
contextPrototype.M = moduleFactories;
const availableModules = new Map();
const availableModuleChunks = new Map();
function loadChunk(chunkData) {
    return loadChunkInternal(SourceType.Parent, this.m.id, chunkData);
}
browserContextPrototype.l = loadChunk;
function loadInitialChunk(chunkPath, chunkData) {
    return loadChunkInternal(SourceType.Runtime, chunkPath, chunkData);
}
async function loadChunkInternal(sourceType, sourceData, chunkData) {
    if (typeof chunkData === 'string') {
        return loadChunkPath(sourceType, sourceData, chunkData);
    }
    const includedList = chunkData.included || [];
    const modulesPromises = includedList.map((included)=>{
        if (moduleFactories.has(included)) return true;
        return availableModules.get(included);
    });
    if (modulesPromises.length > 0 && modulesPromises.every((p)=>p)) {
        // When all included items are already loaded or loading, we can skip loading ourselves
        await Promise.all(modulesPromises);
        return;
    }
    const includedModuleChunksList = chunkData.moduleChunks || [];
    const moduleChunksPromises = includedModuleChunksList.map((included)=>{
        // TODO(alexkirsz) Do we need this check?
        // if (moduleFactories[included]) return true;
        return availableModuleChunks.get(included);
    }).filter((p)=>p);
    let promise;
    if (moduleChunksPromises.length > 0) {
        // Some module chunks are already loaded or loading.
        if (moduleChunksPromises.length === includedModuleChunksList.length) {
            // When all included module chunks are already loaded or loading, we can skip loading ourselves
            await Promise.all(moduleChunksPromises);
            return;
        }
        const moduleChunksToLoad = new Set();
        for (const moduleChunk of includedModuleChunksList){
            if (!availableModuleChunks.has(moduleChunk)) {
                moduleChunksToLoad.add(moduleChunk);
            }
        }
        for (const moduleChunkToLoad of moduleChunksToLoad){
            const promise = loadChunkPath(sourceType, sourceData, moduleChunkToLoad);
            availableModuleChunks.set(moduleChunkToLoad, promise);
            moduleChunksPromises.push(promise);
        }
        promise = Promise.all(moduleChunksPromises);
    } else {
        promise = loadChunkPath(sourceType, sourceData, chunkData.path);
        // Mark all included module chunks as loading if they are not already loaded or loading.
        for (const includedModuleChunk of includedModuleChunksList){
            if (!availableModuleChunks.has(includedModuleChunk)) {
                availableModuleChunks.set(includedModuleChunk, promise);
            }
        }
    }
    for (const included of includedList){
        if (!availableModules.has(included)) {
            // It might be better to race old and new promises, but it's rare that the new promise will be faster than a request started earlier.
            // In production it's even more rare, because the chunk optimization tries to deduplicate modules anyway.
            availableModules.set(included, promise);
        }
    }
    await promise;
}
const loadedChunk = Promise.resolve(undefined);
const instrumentedBackendLoadChunks = new WeakMap();
// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkByUrl(chunkUrl) {
    return loadChunkByUrlInternal(SourceType.Parent, this.m.id, chunkUrl);
}
browserContextPrototype.L = loadChunkByUrl;
const loadedScripts = new Map();
/**
 * Load an external script by creating a <script> tag.
 * This is used for script externals that need to be loaded from CDN or other external sources.
 */ function loadScript(scriptUrl) {
    // Return cached promise if script is already loading or loaded
    let promise = loadedScripts.get(scriptUrl);
    if (promise) {
        return promise;
    }
    promise = new Promise((resolve, reject)=>{
        const script = document.createElement('script');
        script.crossOrigin = CROSS_ORIGIN;
        script.src = scriptUrl;
        script.onload = ()=>resolve();
        script.onerror = ()=>reject(new Error(`Failed to load script: ${scriptUrl}`));
        document.head.appendChild(script);
    });
    loadedScripts.set(scriptUrl, promise);
    return promise;
}
browserContextPrototype.S = loadScript;
// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkByUrlInternal(sourceType, sourceData, chunkUrl) {
    const thenable = BACKEND.loadChunkCached(sourceType, chunkUrl);
    let entry = instrumentedBackendLoadChunks.get(thenable);
    if (entry === undefined) {
        const resolve = instrumentedBackendLoadChunks.set.bind(instrumentedBackendLoadChunks, thenable, loadedChunk);
        entry = thenable.then(resolve).catch((cause)=>{
            let loadReason;
            switch(sourceType){
                case SourceType.Runtime:
                    loadReason = `as a runtime dependency of chunk ${sourceData}`;
                    break;
                case SourceType.Parent:
                    loadReason = `from module ${sourceData}`;
                    break;
                case SourceType.Update:
                    loadReason = 'from an HMR update';
                    break;
                default:
                    invariant(sourceType, (sourceType)=>`Unknown source type: ${sourceType}`);
            }
            let error = new Error(`Failed to load chunk ${chunkUrl} ${loadReason}${cause ? `: ${cause}` : ''}`, cause ? {
                cause
            } : undefined);
            error.name = 'ChunkLoadError';
            throw error;
        });
        instrumentedBackendLoadChunks.set(thenable, entry);
    }
    return entry;
}
// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkPath(sourceType, sourceData, chunkPath) {
    const url = getChunkRelativeUrl(chunkPath);
    return loadChunkByUrlInternal(sourceType, sourceData, url);
}
/**
 * Returns an absolute url to an asset.
 */ function resolvePathFromModule(moduleId) {
    const exported = this.r(moduleId);
    return exported?.default ?? exported;
}
browserContextPrototype.R = resolvePathFromModule;
/**
 * no-op for browser
 * @param modulePath
 */ function resolveAbsolutePath(modulePath) {
    return `/ROOT/${modulePath ?? ''}`;
}
browserContextPrototype.P = resolveAbsolutePath;
/**
 * Returns a placeholder `file://` URL for the given module path. The browser
 * runtime intentionally does not expose the real filesystem path. Path
 * segments are percent-encoded so the result is always a valid file URI.
 */ function resolveFileUrl(modulePath) {
    if (!modulePath) return 'file:///ROOT/';
    return `file:///ROOT/${modulePath.split('/').map(encodeURIComponent).join('/')}`;
}
browserContextPrototype.F = resolveFileUrl;
/**
 * Exports a URL with the static suffix appended.
 */ function exportUrl(url, id) {
    exportValue.call(this, `${url}${ASSET_SUFFIX}`, id);
}
browserContextPrototype.q = exportUrl;
/**
 * Instantiates a runtime module.
 */ function instantiateRuntimeModule(moduleId, chunkPath) {
    return instantiateModule(moduleId, SourceType.Runtime, chunkPath);
}
/**
 * Returns the URL relative to the origin where a chunk can be fetched from.
 */ function getChunkRelativeUrl(chunkPath, basePath = CHUNK_BASE_PATH) {
    return `${getRuntimeChunkBasePath(basePath)}${chunkPath.split('/').map((p)=>encodeURIComponent(p)).join('/')}${ASSET_SUFFIX}`;
}
// Shared runtime primitives consumed by the bundled `createWorker` helper,
// exposed as `__turbopack_chunk_base_path__` and `__turbopack_chunk_asset_suffix__`.
browserContextPrototype.b = CHUNK_BASE_PATH;
browserContextPrototype.X = ASSET_SUFFIX;
// Shared runtime primitive: build a chunk's URL. Used by the bundled worker
// helper and the WASM helper, exposed as `__turbopack_chunk_relative_url__`.
browserContextPrototype.h = getChunkRelativeUrl;
function getPathFromScript(chunkScript) {
    if (typeof chunkScript === 'string') {
        return chunkScript;
    }
    const chunkUrl = chunkScript.src;
    const src = decodeURIComponent(chunkUrl.replace(/[?#].*$/, ''));
    const runtimeBasePath = getRuntimeChunkBasePath();
    let path = src.startsWith(runtimeBasePath) ? src.slice(runtimeBasePath.length) : src;
    if (path.startsWith('/')) {
        path = path.slice(1);
    }
    return path;
}
/**
 * Return the ChunkUrl from a ChunkScript.
 */ function getUrlFromScript(chunk) {
    if (typeof chunk === 'string') {
        return getChunkRelativeUrl(chunk);
    } else {
        // This is already exactly what we want
        return chunk.src;
    }
}
/**
 * Determine the chunk to register. Note that this function has side-effects!
 */ function getChunkFromRegistration(chunk) {
    if (typeof chunk === 'string') {
        return chunk;
    } else if (!chunk) {
        if (typeof TURBOPACK_NEXT_CHUNK_URLS !== 'undefined') {
            return {
                src: TURBOPACK_NEXT_CHUNK_URLS.pop()
            };
        } else {
            throw new Error('chunk path empty but not in a worker');
        }
    } else {
        return {
            src: chunk.getAttribute('src')
        };
    }
}
/**
 * Checks if a given path/URL ends with the given extension,
 * optionally followed by ?query or #fragment.
 */ function endsWithExtension(chunkUrlOrPath, ext) {
    // Find where the path ends (before query or fragment)
    const q = chunkUrlOrPath.indexOf('?');
    let end;
    if (q !== -1) {
        end = q;
    } else {
        const h = chunkUrlOrPath.indexOf('#');
        end = h !== -1 ? h : chunkUrlOrPath.length;
    }
    // Check if the path portion ends with the extension
    return end >= ext.length && chunkUrlOrPath.startsWith(ext, end - ext.length);
}
function isJs(chunkUrlOrPath) {
    return endsWithExtension(chunkUrlOrPath, '.js');
}
function isCss(chunkUrl) {
    return endsWithExtension(chunkUrl, '.css');
}
/// <reference path="./runtime-base.ts" />
/// <reference path="./dummy.ts" />
const moduleCache = {};
contextPrototype.c = moduleCache;
/**
 * Gets or instantiates a runtime module.
 */ // @ts-ignore
// eslint-disable-next-line @typescript-eslint/no-unused-vars
function getOrInstantiateRuntimeModule(chunkPath, moduleId) {
    const module = moduleCache[moduleId];
    if (module) {
        if (module.error) {
            throw module.error;
        }
        return module;
    }
    return instantiateModule(moduleId, SourceType.Runtime, chunkPath);
}
/**
 * Retrieves a module from the cache, or instantiate it if it is not cached.
 */ // Used by the backend
// @ts-ignore
// eslint-disable-next-line @typescript-eslint/no-unused-vars
const getOrInstantiateModuleFromParent = (id, sourceModule)=>{
    const module = moduleCache[id];
    if (module) {
        if (module.error) {
            throw module.error;
        }
        return module;
    }
    return instantiateModule(id, SourceType.Parent, sourceModule.id);
};
function instantiateModule(id, sourceType, sourceData) {
    const moduleFactory = moduleFactories.get(id);
    if (typeof moduleFactory !== 'function') {
        // This can happen if modules incorrectly handle HMR disposes/updates,
        // e.g. when they keep a `setTimeout` around which still executes old code
        // and contains e.g. a `require("something")` call.
        throw new Error(factoryNotAvailableMessage(id, sourceType, sourceData));
    }
    const module = createModuleObject(id);
    const exports = module.exports;
    moduleCache[id] = module;
    // NOTE(alexkirsz) This can fail when the module encounters a runtime error.
    const context = new Context(module, exports);
    try {
        moduleFactory(context, module, exports);
    } catch (error) {
        module.error = error;
        throw error;
    }
    if (module.namespaceObject && module.exports !== module.namespaceObject) {
        // in case of a circular dependency: cjs1 -> esm2 -> cjs1
        interopEsm(module.exports, module.namespaceObject);
    }
    return module;
}
// eslint-disable-next-line @typescript-eslint/no-unused-vars
function registerChunk(registration) {
    const chunk = getChunkFromRegistration(registration[0]);
    let runtimeParams;
    // When bootstrapping we are passed a single runtimeParams object so we can distinguish purely based on length
    if (registration.length === 2) {
        runtimeParams = registration[1];
    } else {
        runtimeParams = undefined;
        installCompressedModuleFactories(registration, /* offset= */ 1, moduleFactories);
    }
    return BACKEND.registerChunk(chunk, runtimeParams);
}
/**
 * This file contains the runtime code specific to the Turbopack ECMAScript DOM runtime.
 *
 * It will be appended to the base runtime code.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="../../../browser/runtime/base/runtime-base.ts" />
/// <reference path="../../../shared/runtime/runtime-types.d.ts" />
function getAssetSuffixFromScriptSrc() {
    // TURBOPACK_ASSET_SUFFIX is set in web workers
    if (self.TURBOPACK_ASSET_SUFFIX != null) return self.TURBOPACK_ASSET_SUFFIX;
    const src = document?.currentScript?.getAttribute?.('src') ?? '';
    const qi = src.indexOf('?');
    return qi >= 0 ? src.slice(qi) : '';
}
let BACKEND;
/**
 * Maps chunk paths to the corresponding resolver.
 */ const chunkResolvers = new Map();
(()=>{
    BACKEND = {
        async registerChunk (chunk, params) {
            let chunkPath = getPathFromScript(chunk);
            let chunkUrl = getUrlFromScript(chunkPath);
            const resolver = getOrCreateResolver(chunkUrl);
            resolver.resolve();
            if (params == null) {
                return;
            }
            for (const otherChunkData of params.otherChunks){
                const otherChunkPath = getChunkPath(otherChunkData);
                const otherChunkUrl = getChunkRelativeUrl(otherChunkPath);
                // Chunk might have started loading, so we want to avoid triggering another load.
                getOrCreateResolver(otherChunkUrl);
            }
            // This waits for chunks to be loaded, but also marks included items as available.
            await Promise.all(params.otherChunks.map((otherChunkData)=>loadInitialChunk(chunkPath, otherChunkData)));
            if (params.runtimeModuleIds.length > 0) {
                for (const moduleId of params.runtimeModuleIds){
                    getOrInstantiateRuntimeModule(chunkPath, moduleId);
                }
            }
        },
        /**
     * Loads the given chunk, and returns a promise that resolves once the chunk
     * has been loaded.
     */ loadChunkCached (sourceType, chunkUrl) {
            return doLoadChunk(sourceType, chunkUrl);
        }
    };
    function getOrCreateResolver(chunkUrl) {
        let resolver = chunkResolvers.get(chunkUrl);
        if (!resolver) {
            let resolve;
            let reject;
            const promise = new Promise((innerResolve, innerReject)=>{
                resolve = innerResolve;
                reject = innerReject;
            });
            resolver = {
                resolved: false,
                loadingStarted: false,
                retryAttempts: 0,
                promise,
                resolve: ()=>{
                    resolver.resolved = true;
                    resolve();
                },
                reject: reject
            };
            chunkResolvers.set(chunkUrl, resolver);
        }
        return resolver;
    }
    /**
   * Rejects a chunk resolver and drops it from the cache.
   * We don't want to cache failed chunk loads: a later
   * request for the same chunk should try again.
   */ function rejectChunkResolver(chunkUrl, resolver, error) {
        if (chunkResolvers.get(chunkUrl) === resolver) {
            chunkResolvers.delete(chunkUrl);
        }
        resolver.reject(error);
    }
    function getChunkLoadRetryDelayMs() {
        const jitter = Math.floor(Math.random() * (CHUNK_LOAD_RETRY_MAX_JITTER_MS + 1));
        return CHUNK_LOAD_RETRY_BASE_DELAY_MS + jitter;
    }
    function isRetryableChunkLoadError(error) {
        return error == null || error instanceof DOMException && error.name === 'NetworkError';
    }
    /**
   * Handles a failed chunk load: retries the load once after a short delay.
   */ function onChunkLoadError(sourceType, chunkUrl, resolver, error, reload) {
        if (!isRetryableChunkLoadError(error) || resolver.retryAttempts >= CHUNK_LOAD_RETRY_MAX_ATTEMPTS || chunkResolvers.get(chunkUrl) !== resolver) {
            rejectChunkResolver(chunkUrl, resolver, error);
            return;
        }
        resolver.retryAttempts++;
        setTimeout(()=>{
            // if this chunk is being fetched multiple times, and one of those
            // attempts succeeds. or, if this chunk has another resolver
            // mapped to it - it's safe to skip retrying.
            if (resolver.resolved || chunkResolvers.get(chunkUrl) !== resolver) {
                return;
            }
            if (reload) {
                reload();
            } else {
                resolver.loadingStarted = false;
                doLoadChunk(sourceType, chunkUrl);
            }
        }, getChunkLoadRetryDelayMs());
    }
    /**
   * Loads the given chunk, and returns a promise that resolves once the chunk
   * has been loaded.
   */ function doLoadChunk(sourceType, chunkUrl) {
        const resolver = getOrCreateResolver(chunkUrl);
        if (resolver.loadingStarted) {
            return resolver.promise;
        }
        if (sourceType === SourceType.Runtime) {
            // CSS chunks do not register themselves, and as such must be marked as
            // loaded instantly.
            resolver.loadingStarted = true;
            if (isCss(chunkUrl)) {
                if (typeof importScripts !== 'function') {
                    const decodedChunkUrl = decodeURI(chunkUrl);
                    const previousLinks = document.querySelectorAll(`link[rel=stylesheet][href="${chunkUrl}"],link[rel=stylesheet][href^="${chunkUrl}?"],link[rel=stylesheet][href="${decodedChunkUrl}"],link[rel=stylesheet][href^="${decodedChunkUrl}?"]`);
                    if (previousLinks.length === 0) {
                        const link = document.createElement('link');
                        link.rel = 'stylesheet';
                        link.crossOrigin = CROSS_ORIGIN;
                        link.href = chunkUrl;
                        link.onerror = ()=>{
                            resolver.reject();
                        };
                        link.onload = ()=>{
                            resolver.resolve();
                        };
                        document.head.appendChild(link);
                        return resolver.promise;
                    }
                }
                resolver.resolve();
                return resolver.promise;
            }
            // Runtime JS chunks are expected to be present in the DOM already.
            // Load it first
            if (typeof importScripts !== 'function') {
                const decodedChunkUrl = decodeURI(chunkUrl);
                const previousScripts = document.querySelectorAll(`script[src="${chunkUrl}"],script[src^="${chunkUrl}?"],script[src="${decodedChunkUrl}"],script[src^="${decodedChunkUrl}?"]`);
                if (previousScripts.length > 0) {
                    for (const script of Array.from(previousScripts)){
                        script.addEventListener('error', ()=>{
                            resolver.reject();
                        });
                    }
                    return resolver.promise;
                }
            }
        // If it wasn't present in the DOM, fallback to loading logic.
        }
        if (typeof importScripts === 'function') {
            // We're in a web worker
            if (isCss(chunkUrl)) {
            // ignore
            } else if (isJs(chunkUrl)) {
                self.TURBOPACK_NEXT_CHUNK_URLS.push(chunkUrl);
                try {
                    importScripts(chunkUrl);
                } catch (error) {
                    onChunkLoadError(sourceType, chunkUrl, resolver, error);
                }
            } else {
                throw new Error(`can't infer type of chunk from URL ${chunkUrl} in worker`);
            }
        } else {
            // TODO(PACK-2140): remove this once all filenames are guaranteed to be escaped.
            const decodedChunkUrl = decodeURI(chunkUrl);
            if (isCss(chunkUrl)) {
                const previousLinks = document.querySelectorAll(`link[rel=stylesheet][href="${chunkUrl}"],link[rel=stylesheet][href^="${chunkUrl}?"],link[rel=stylesheet][href="${decodedChunkUrl}"],link[rel=stylesheet][href^="${decodedChunkUrl}?"]`);
                if (previousLinks.length > 0) {
                    // CSS chunks do not register themselves, and as such must be marked as
                    // loaded instantly.
                    resolver.resolve();
                } else {
                    const createLink = ()=>{
                        const link = document.createElement('link');
                        link.rel = 'stylesheet';
                        link.crossOrigin = CROSS_ORIGIN;
                        link.href = chunkUrl;
                        link.onerror = ()=>{
                            // Re-insert a fresh tag at the same position on retry to preserve
                            // cascade order.
                            const anchor = document.createComment('');
                            link.replaceWith(anchor);
                            onChunkLoadError(sourceType, chunkUrl, resolver, undefined, ()=>anchor.replaceWith(createLink()));
                        };
                        link.onload = ()=>{
                            // CSS chunks do not register themselves, and as such must be marked as
                            // loaded instantly.
                            resolver.resolve();
                        };
                        return link;
                    };
                    // Append to the `head` for webpack compatibility.
                    document.head.appendChild(createLink());
                }
            } else if (isJs(chunkUrl)) {
                const previousScripts = document.querySelectorAll(`script[src="${chunkUrl}"],script[src^="${chunkUrl}?"],script[src="${decodedChunkUrl}"],script[src^="${decodedChunkUrl}?"]`);
                if (previousScripts.length > 0) {
                    for (const script of Array.from(previousScripts)){
                        script.addEventListener('error', ()=>{
                            // Drop the failed tag so a retry can re-add it cleanly.
                            script.remove();
                            onChunkLoadError(sourceType, chunkUrl, resolver);
                        }, {
                            once: true
                        });
                    }
                } else {
                    const script = document.createElement('script');
                    script.crossOrigin = CROSS_ORIGIN;
                    script.src = chunkUrl;
                    // We'll only mark the chunk as loaded once the script has been executed,
                    // which happens in `registerChunk`. Hence the absence of `resolve()` in
                    // this branch.
                    script.onerror = ()=>{
                        // Drop the failed tag so a retry can re-add it cleanly.
                        script.remove();
                        onChunkLoadError(sourceType, chunkUrl, resolver);
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
var chunksToRegister = globalThis["utooChunk_export_entry_exports"];
globalThis["utooChunk_export_entry_exports"] = { push: registerChunk };
chunksToRegister.forEach(registerChunk);

try {
for (const registration of chunksToRegister) {
    const runtimeParams = registration.length === 2 ? registration[1] : null;
    if (runtimeParams && runtimeParams.runtimeModuleIds && runtimeParams.runtimeModuleIds.length > 0) {
        const entryModuleId = runtimeParams.runtimeModuleIds[runtimeParams.runtimeModuleIds.length - 1];
        const chunkPath = getPathFromScript(registration[0]);

        const entryModule = getOrInstantiateRuntimeModule(chunkPath, entryModuleId);

        if (entryModule && entryModule.exports) {
            const moduleExports = entryModule.namespaceObject || entryModule.exports;

            // Save for return value (will be handled by UMD wrapper)
            __entryExports__ = moduleExports;
        }
        break;
    }
}
} catch (e) {
    console.error('Failed to expose entry module exports:', e);
}
    return __entryExports__;
})();

// Return the exports from the factory function
return __chunk__;
}));


//# sourceMappingURL=main.js.map