((__UTOOPACK__) => {
            
if (!Array.isArray(__UTOOPACK__)) {
    return;
}

const CHUNK_BASE_PATH = "";
const CHUNK_SUFFIX_PATH = "";
const RELATIVE_ROOT_PATH = "..";
const RUNTIME_PUBLIC_PATH = "";
/**
 * This file contains runtime types and functions that are shared between all
 * TurboPack ECMAScript runtimes.
 *
 * It will be prepended to the runtime code of each runtime.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./runtime-types.d.ts" />
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
const toStringTag = typeof Symbol !== "undefined" && Symbol.toStringTag;
function defineProp(obj, name, options) {
    if (!hasOwnProperty.call(obj, name)) Object.defineProperty(obj, name, options);
}
function getOverwrittenModule(moduleCache, id) {
    let module = moduleCache[id];
    if (!module) {
        // This is invoked when a module is merged into another module, thus it wasn't invoked via
        // instantiateModule and the cache entry wasn't created yet.
        module = createModuleObject(id);
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
const BindingTag_Value = 0;
/**
 * Adds the getters to the exports object.
 */ function esm(exports, bindings) {
    defineProp(exports, "__esModule", {
        value: true
    });
    if (toStringTag) defineProp(exports, toStringTag, {
        value: "Module"
    });
    let i = 0;
    while(i < bindings.length){
        const propName = bindings[i++];
        const tagOrFunction = bindings[i++];
        if (typeof tagOrFunction === "number") {
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
            if (typeof bindings[i] === "function") {
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
                if (hasOwnProperty.call(target, prop) || prop === "default" || prop === "__esModule") {
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
                        if (key !== "default" && !keys.includes(key)) keys.push(key);
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
    if (typeof object === "object" && object !== null) {
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
    for(let current = raw; (typeof current === "object" || typeof current === "function") && !LEAF_PROTOTYPES.includes(current); current = getProto(current)){
        for (const key of Object.getOwnPropertyNames(current)){
            bindings.push(key, createGetter(raw, key));
            if (defaultLocation === -1 && key === "default") {
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
            bindings.push("default", BindingTag_Value, raw);
        }
    }
    esm(ns, bindings);
    return ns;
}
function createNS(raw) {
    if (typeof raw === "function") {
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
typeof require === "function" ? require : function require1() {
    throw new Error("Unexpected use of runtime require");
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
    const hashIndex = request.indexOf("#");
    if (hashIndex !== -1) {
        request = request.substring(0, hashIndex);
    }
    const queryIndex = request.indexOf("?");
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
        e.code = "MODULE_NOT_FOUND";
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
        e.code = "MODULE_NOT_FOUND";
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
    return typeof chunkData === "string" ? chunkData : chunkData.path;
}
function isPromise(maybePromise) {
    return maybePromise != null && typeof maybePromise === "object" && "then" in maybePromise && typeof maybePromise.then === "function";
}
function isAsyncModuleExt(obj) {
    return turbopackQueues in obj;
}
function createPromise() {
    let resolve;
    let reject;
    const promise = new Promise((res, rej)=>{
        reject = rej;
        resolve = res;
    });
    return {
        promise,
        resolve: resolve,
        reject: reject
    };
}
// everything below is adapted from webpack
// https://github.com/webpack/webpack/blob/6be4065ade1e252c1d8dcba4af0f43e32af1bdc1/lib/runtime/AsyncModuleRuntimeModule.js#L13
const turbopackQueues = Symbol("turbopack queues");
const turbopackExports = Symbol("turbopack exports");
const turbopackError = Symbol("turbopack error");
function resolveQueue(queue) {
    if (queue && queue.status !== 1) {
        queue.status = 1;
        queue.forEach((fn)=>fn.queueCount--);
        queue.forEach((fn)=>fn.queueCount-- ? fn.queueCount++ : fn());
    }
}
function wrapDeps(deps) {
    return deps.map((dep)=>{
        if (dep !== null && typeof dep === "object") {
            if (isAsyncModuleExt(dep)) return dep;
            if (isPromise(dep)) {
                const queue = Object.assign([], {
                    status: 0
                });
                const obj = {
                    [turbopackExports]: {},
                    [turbopackQueues]: (fn)=>fn(queue)
                };
                dep.then((res)=>{
                    obj[turbopackExports] = res;
                    resolveQueue(queue);
                }, (err)=>{
                    obj[turbopackError] = err;
                    resolveQueue(queue);
                });
                return obj;
            }
        }
        return {
            [turbopackExports]: dep,
            [turbopackQueues]: ()=>{}
        };
    });
}
function asyncModule(body, hasAwait) {
    const module = this.m;
    const queue = hasAwait ? Object.assign([], {
        status: -1
    }) : undefined;
    const depQueues = new Set();
    const { resolve, reject, promise: rawPromise } = createPromise();
    const promise = Object.assign(rawPromise, {
        [turbopackExports]: module.exports,
        [turbopackQueues]: (fn)=>{
            queue && fn(queue);
            depQueues.forEach(fn);
            promise["catch"](()=>{});
        }
    });
    const attributes = {
        get () {
            return promise;
        },
        set (v) {
            // Calling `esmExport` leads to this.
            if (v !== promise) {
                promise[turbopackExports] = v;
            }
        }
    };
    Object.defineProperty(module, "exports", attributes);
    Object.defineProperty(module, "namespaceObject", attributes);
    function handleAsyncDependencies(deps) {
        const currentDeps = wrapDeps(deps);
        const getResult = ()=>currentDeps.map((d)=>{
                if (d[turbopackError]) throw d[turbopackError];
                return d[turbopackExports];
            });
        const { promise, resolve } = createPromise();
        const fn = Object.assign(()=>resolve(getResult), {
            queueCount: 0
        });
        function fnQueue(q) {
            if (q !== queue && !depQueues.has(q)) {
                depQueues.add(q);
                if (q && q.status === 0) {
                    fn.queueCount++;
                    q.push(fn);
                }
            }
        }
        currentDeps.map((dep)=>dep[turbopackQueues](fnQueue));
        return fn.queueCount ? promise : getResult();
    }
    function asyncResult(err) {
        if (err) {
            reject(promise[turbopackError] = err);
        } else {
            resolve(promise[turbopackExports]);
        }
        resolveQueue(queue);
    }
    body(handleAsyncDependencies, asyncResult);
    if (queue && queue.status === -1) {
        queue.status = 0;
    }
}
contextPrototype.a = asyncModule;
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
    const realUrl = new URL(inputUrl, "x:/");
    const values = {};
    for(const key in realUrl)values[key] = realUrl[key];
    values.href = inputUrl;
    values.pathname = inputUrl.replace(/[?#].*/, "");
    values.origin = values.protocol = "";
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
 * A stub function to make `require` available but non-functional in ESM.
 */ function requireStub(_moduleId) {
    throw new Error("dynamic usage of require is not supported");
}
contextPrototype.z = requireStub;
// Make `globalThis` available to the module in a way that cannot be shadowed by a local variable.
contextPrototype.g = globalThis;
/**
 * Gets the public path for runtime assets.
 * Checks globalThis.publicPath and falls back to empty string.
 */ function getPublicPath() {
    if (typeof globalThis !== "undefined" && typeof globalThis.publicPath === "string") {
        const publicPath = globalThis.publicPath;
        return publicPath.endsWith("/") ? publicPath : `${publicPath}/`;
    }
    return "";
}
contextPrototype.p = getPublicPath;
/**
 * This file contains runtime types and functions that are shared between all
 * Turbopack *development* ECMAScript runtimes.
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./globals.d.ts" />
/// <reference path="./runtime-utils.ts" />
// Used in WebWorkers to tell the runtime about the chunk base path
const browserContextPrototype = Context.prototype;
var SourceType = /*#__PURE__*/ function(SourceType) {
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
const moduleFactories = new Map();
contextPrototype.M = moduleFactories;
const availableModules = new Map();
const availableModuleChunks = new Map();
function factoryNotAvailable(moduleId, sourceType, sourceData) {
    let instantiationReason;
    switch(sourceType){
        case 0:
            instantiationReason = `as a runtime entry of chunk ${sourceData}`;
            break;
        case 1:
            instantiationReason = `because it was required from module ${sourceData}`;
            break;
        case 2:
            instantiationReason = "because of an HMR update";
            break;
        default:
            invariant(sourceType, (sourceType)=>`Unknown source type: ${sourceType}`);
    }
    throw new Error(`Module ${moduleId} was instantiated ${instantiationReason}, but the module factory is not available. It might have been deleted in an HMR update.`);
}
const loadedChunk = Promise.resolve(undefined);
const instrumentedBackendLoadChunks = new WeakMap();
// Do not make this async. React relies on referential equality of the returned Promise.
function loadChunkByUrl(chunkUrl) {
    return loadChunkByUrlInternal(1, this.m.id, chunkUrl);
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
        const script = document.createElement("script");
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
    const thenable = BACKEND.loadChunkCached(sourceType, sourceData, chunkUrl);
    let entry = instrumentedBackendLoadChunks.get(thenable);
    if (entry === undefined) {
        const resolve = instrumentedBackendLoadChunks.set.bind(instrumentedBackendLoadChunks, thenable, loadedChunk);
        entry = thenable.then(resolve).catch((error)=>{
            let loadReason;
            switch(sourceType){
                case 0:
                    loadReason = `as a runtime dependency of chunk ${sourceData}`;
                    break;
                case 1:
                    loadReason = `from module ${sourceData}`;
                    break;
                case 2:
                    loadReason = "from an HMR update";
                    break;
                default:
                    invariant(sourceType, (sourceType)=>`Unknown source type: ${sourceType}`);
            }
            throw new Error(`Failed to load chunk ${chunkUrl} ${loadReason}${error ? `: ${error}` : ""}`, error ? {
                cause: error
            } : undefined);
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
    return `/ROOT/${modulePath ?? ""}`;
}
browserContextPrototype.P = resolveAbsolutePath;
/**
 * Instantiates a runtime module.
 */ function instantiateRuntimeModule(moduleId, chunkPath) {
    return instantiateModule(moduleId, 0, chunkPath);
}
/**
 * Returns the URL relative to the origin where a chunk can be fetched from.
 */ function getChunkRelativeUrl(chunkPath) {
    return `${CHUNK_BASE_PATH}${chunkPath.split("/").map((p)=>encodeURIComponent(p)).join("/")}${CHUNK_SUFFIX_PATH}`;
}
function getPathFromScript(chunkScript) {
    if (typeof chunkScript === "string") {
        return chunkScript;
    }
    const chunkUrl = typeof TURBOPACK_NEXT_CHUNK_URLS !== "undefined" ? TURBOPACK_NEXT_CHUNK_URLS.pop() : chunkScript.getAttribute("src");
    const src = decodeURIComponent(chunkUrl.replace(/[?#].*$/, ""));
    let path = src.startsWith(CHUNK_BASE_PATH) ? src.slice(CHUNK_BASE_PATH.length) : src;
    if (path.startsWith("/")) {
        path = path.slice(1);
    }
    return path;
}
function registerCompressedModuleFactory(moduleId, moduleFactory) {
    if (!moduleFactories.has(moduleId)) {
        if (Array.isArray(moduleFactory)) {
            let [moduleFactoryFn, otherIds] = moduleFactory;
            moduleFactories.set(moduleId, moduleFactoryFn);
            for (const otherModuleId of otherIds){
                moduleFactories.set(otherModuleId, moduleFactoryFn);
            }
        } else {
            moduleFactories.set(moduleId, moduleFactory);
        }
    }
}
const regexJsUrl = /\.js(?:\?[^#]*)?(?:#.*)?$/;
/**
 * Checks if a given path/URL ends with .js, optionally followed by ?query or #fragment.
 */ function isJs(chunkUrlOrPath) {
    return regexJsUrl.test(chunkUrlOrPath);
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
    if (typeof moduleFactory !== "function") {
        // This can happen if modules incorrectly handle HMR disposes/updates,
        // e.g. when they keep a `setTimeout` around which still executes old code
        // and contains e.g. a `require("something")` call.
        factoryNotAvailable(id, sourceType, sourceData);
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
function registerChunk([chunkScript, chunkModules, runtimeParams]) {
    const chunkPath = getPathFromScript(chunkScript);
    for (const [moduleId, moduleFactory] of Object.entries(chunkModules)){
        registerCompressedModuleFactory(moduleId, moduleFactory);
    }
    return BACKEND.registerChunk(chunkPath, runtimeParams);
}
/* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./runtime-utils.ts" />
/// A 'base' utilities to support runtime can have externals.
/// Currently this is for node.js / edge runtime both.
/// If a fn requires node.js specific behavior, it should be placed in `node-external-utils` instead.
async function externalImport(id) {
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
function externalRequire(id, thunk, esm = false) {
    let raw;
    try {
        raw = thunk();
    } catch (err) {
        // TODO(alexkirsz) This can happen when a client-side module tries to load
        // an external module we don't provide a shim for (e.g. querystring, url).
        // For now, we fail semi-silently, but in the future this should be a
        // compilation error.
        throw new Error(`Failed to load external module ${id}: ${err}`);
    }
    if (!esm || raw.__esModule) {
        return raw;
    }
    return interopEsm(raw, createNS(raw), true);
}
externalRequire.resolve = (id, options)=>{
    return require.resolve(id, options);
};
contextPrototype.x = externalRequire;
/**
 * This file contains the runtime code specific to the Turbopack development
 * ECMAScript DOM runtime.
 *
 * It will be appended to the base development runtime code.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./runtime-base.ts" />
/// <reference path="./runtime-types.d.ts" />
let BACKEND;
/**
 * Maps chunk paths to the corresponding resolver.
 */ const chunkResolvers = new Map();
(()=>{
    BACKEND = {
        registerChunk (chunkPath, params) {
            const chunkUrl = getChunkRelativeUrl(chunkPath);
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
            if (params.runtimeModuleIds.length > 0) {
                for (const moduleId of params.runtimeModuleIds){
                    getOrInstantiateRuntimeModule(chunkPath, moduleId);
                }
            }
        },
        /**
     * Loads the given chunk, and returns a promise that resolves once the chunk
     * has been loaded.
     */ loadChunkCached (sourceType, sourceData, chunkUrl) {
            return doLoadChunk(sourceType, sourceData, chunkUrl);
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
   * Loads the given chunk, and returns a promise that resolves once the chunk
   * has been loaded.
   */ function doLoadChunk(sourceType, _sourceData, chunkUrl) {
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
                self.TURBOPACK_NEXT_CHUNK_URLS.push(chunkUrl);
                importScripts(TURBOPACK_WORKER_LOCATION + chunkUrl);
            } else {
                throw new Error(`can't infer type of chunk from URL ${chunkUrl} in worker`);
            }
        } else {
            // TODO(PACK-2140): remove this once all filenames are guaranteed to be escaped.
            const decodedChunkUrl = decodeURI(chunkUrl);
            if (isJs(chunkUrl)) {
                const previousScripts = document.querySelectorAll(`script[src="${chunkUrl}"],script[src^="${chunkUrl}?"],script[src="${decodedChunkUrl}"],script[src^="${decodedChunkUrl}?"]`);
                if (previousScripts.length > 0) {
                    // There is this edge where the script already failed loading, but we
                    // can't detect that. The Promise will never resolve in this case.
                    for (const script of Array.from(previousScripts)){
                        script.addEventListener("error", ()=>{
                            resolver.reject();
                        });
                    }
                } else {
                    const script = document.createElement("script");
                    script.src = chunkUrl;
                    // We'll only mark the chunk as loaded once the script has been executed,
                    // which happens in `registerChunk`. Hence the absence of `resolve()` in
                    // this branch.
                    script.onerror = ()=>{
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
const chunksToRegister = __UTOOPACK__;
__UTOOPACK__ = { push: registerChunk };
chunksToRegister.forEach(registerChunk);
function factory () {
    const runtimeModuleIds = ["[project]/runtime/library_build_runtime/input/index.js [library-client] (ecmascript)"];
    if (runtimeModuleIds.length > 0) {
        const module = moduleCache[runtimeModuleIds[0]];
        if (module.error) throw module.error;
        // any ES module has to have `module.namespaceObject` defined.
        if (module.namespaceObject) return module.namespaceObject;
        // only ESM can be an async module, so we don't need to worry about exports being a promise here.
        const raw = module.exports;
        return module.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
    }
}

if (typeof exports === 'object' && typeof module === 'object') {
    module.exports = factory();
} else if (typeof exports === 'object') {
    exports["MyLibrary"] = factory();
} else {
    globalThis["MyLibrary"] = factory();
}
})([["main.js", {

"[project]/runtime/library_build_runtime/input/index.js [library-client] (ecmascript)": ((__turbopack_context__, module, exports) => {

console.log('Hello, world!');
}),
},
{"otherChunks":[],"runtimeModuleIds":["[project]/runtime/library_build_runtime/input/index.js [library-client] (ecmascript)"]},
]]);


//# sourceMappingURL=main.js.map