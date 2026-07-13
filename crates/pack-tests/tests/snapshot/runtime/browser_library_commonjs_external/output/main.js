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
 * Checks globalThis.publicPath and falls back to "/".
 */ function getPublicPath(mode) {
    if (mode === 'auto') {
        return getAutomaticPublicPath();
    }
    if (typeof globalThis !== 'undefined' && typeof globalThis.publicPath === 'string') {
        const publicPath = globalThis.publicPath;
        return publicPath.endsWith('/') ? publicPath : `${publicPath}/`;
    }
    return '/';
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
 * Turbopack UMD library runtimes (DOM and Node.js).
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-utils.ts" />
/// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-types.d.ts" />
// Provided by build
let BACKEND;
const moduleFactories = new Map();
contextPrototype.M = moduleFactories;
/**
 * Determine the chunk to register from a registration entry.
 * In library builds, chunks are always string paths or script objects.
 */ function getChunkFromRegistration(chunk) {
    if (typeof chunk === "string") {
        return chunk;
    } else if (chunk) {
        return {
            src: chunk.getAttribute("src")
        };
    } else {
        throw new Error("chunk path is empty");
    }
}
/**
 * Load CommonJS externals when a UMD bundle runs in a CommonJS environment.
 * Browser-targeted UMD bundles need this too because their wrapper supports
 * both global and CommonJS consumers.
 */ function externalRequire(id, thunk, esm = false) {
    let raw;
    try {
        raw = thunk();
    } catch (err) {
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
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="./runtime-base.ts" />
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
contextPrototype.S = loadScript;
(()=>{
    BACKEND = {
        registerChunk (chunk, params) {
            if (params == null) {
                return;
            }
            if (params.runtimeModuleIds.length > 0) {
                for (const moduleId of params.runtimeModuleIds){
                    getOrInstantiateRuntimeModule(chunk, moduleId);
                }
            }
        }
    };
})();
const chunksToRegister = __UTOOPACK__;
__UTOOPACK__ = { push: registerChunk };
chunksToRegister.forEach(registerChunk);
function factory () {
    const runtimeModuleIds = ["[project]/runtime/browser_library_commonjs_external/input/index.js [library-client] (ecmascript)"];
    let exports;
    for (let i = 0; i < runtimeModuleIds.length; i++) {
        const module = moduleCache[runtimeModuleIds[i]];
        if (module.error) throw module.error;
        exports = module;
    }
    if (exports) {
        // any ES module has to have `module.namespaceObject` defined.
        if (exports.namespaceObject) return exports.namespaceObject;
        // only ESM can be an async module, so we don't need to worry about exports being a promise here.
        const raw = exports.exports;
        return exports.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
    }
}

if (typeof exports === 'object' && typeof module === 'object') {
    module.exports = factory();
} else if (typeof exports === 'object') {
    exports["BrowserLibrary"] = factory();
} else {
    globalThis["BrowserLibrary"] = factory();
}
})([
["main.js",

"[externals]/root ExternalValue commonjs ../external.cjs [external] (root ExternalValue commonjs ../external.cjs, umd)", ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("../external.cjs", () => require("../external.cjs")); } else { mod = globalThis["ExternalValue"] }

__turbopack_context__.v(mod);
}),
"[project]/runtime/browser_library_commonjs_external/input/index.js [library-client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$externals$5d2f$root__ExternalValue__commonjs__$2e2e2f$external$2e$cjs__$5b$external$5d$__$28$root__ExternalValue__commonjs__$2e2e2f$external$2e$cjs$2c$__umd$29$__ = __turbopack_context__.i("[externals]/root ExternalValue commonjs ../external.cjs [external] (root ExternalValue commonjs ../external.cjs, umd)");
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__$5b$externals$5d2f$root__ExternalValue__commonjs__$2e2e2f$external$2e$cjs__$5b$external$5d$__$28$root__ExternalValue__commonjs__$2e2e2f$external$2e$cjs$2c$__umd$29$__["default"];
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
}),
],
["main.js", {"otherChunks":[],"runtimeModuleIds":["[project]/runtime/browser_library_commonjs_external/input/index.js [library-client] (ecmascript)"]}],
]);


//# sourceMappingURL=main.js.map