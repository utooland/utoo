(function() {
function asyncGeneratorStep(gen, resolve, reject, _next, _throw, key, arg) {
    try {
        var info = gen[key](arg);
        var value = info.value;
    } catch (error) {
        reject(error);
        return;
    }
    if (info.done) resolve(value);
    else Promise.resolve(value).then(_next, _throw);
}
function _async_to_generator(fn) {
    return function() {
        var self1 = this, args = arguments;
        return new Promise(function(resolve, reject) {
            var gen = fn.apply(self1, args);
            function _next(value) {
                asyncGeneratorStep(gen, resolve, reject, _next, _throw, "next", value);
            }
            function _throw(err) {
                asyncGeneratorStep(gen, resolve, reject, _next, _throw, "throw", err);
            }
            _next(undefined);
        });
    };
}
function _define_property(obj, key, value) {
    if (key in obj) {
        Object.defineProperty(obj, key, {
            value: value,
            enumerable: true,
            configurable: true,
            writable: true
        });
    } else obj[key] = value;
    return obj;
}
function _ts_generator(thisArg, body) {
    var f, y, t, _ = {
        label: 0,
        sent: function() {
            if (t[0] & 1) throw t[1];
            return t[1];
        },
        trys: [],
        ops: []
    }, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype), d = Object.defineProperty;
    return d(g, "next", {
        value: verb(0)
    }), d(g, "throw", {
        value: verb(1)
    }), d(g, "return", {
        value: verb(2)
    }), typeof Symbol === "function" && d(g, Symbol.iterator, {
        value: function() {
            return this;
        }
    }), g;
    function verb(n) {
        return function(v) {
            return step([
                n,
                v
            ]);
        };
    }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while(g && (g = 0, op[0] && (_ = 0)), _)try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [
                op[0] & 2,
                t.value
            ];
            switch(op[0]){
                case 0:
                case 1:
                    t = op;
                    break;
                case 4:
                    _.label++;
                    return {
                        value: op[1],
                        done: false
                    };
                case 5:
                    _.label++;
                    y = op[1];
                    op = [
                        0
                    ];
                    continue;
                case 7:
                    op = _.ops.pop();
                    _.trys.pop();
                    continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) {
                        _ = 0;
                        continue;
                    }
                    if (op[0] === 3 && (!t || op[1] > t[0] && op[1] < t[3])) {
                        _.label = op[1];
                        break;
                    }
                    if (op[0] === 6 && _.label < t[1]) {
                        _.label = t[1];
                        t = op;
                        break;
                    }
                    if (t && _.label < t[2]) {
                        _.label = t[2];
                        _.ops.push(op);
                        break;
                    }
                    if (t[2]) _.ops.pop();
                    _.trys.pop();
                    continue;
            }
            op = body.call(thisArg, _);
        } catch (e) {
            op = [
                6,
                e
            ];
            y = 0;
        } finally{
            f = t = 0;
        }
        if (op[0] & 5) throw op[1];
        return {
            value: op[0] ? op[1] : void 0,
            done: true
        };
    }
}
function _type_of(obj) {
    "@swc/helpers - typeof";
    return obj && typeof Symbol !== "undefined" && obj.constructor === Symbol ? "symbol" : typeof obj;
}
(function(__UTOOPACK__) {
    if (typeof globalThis === 'undefined') {
        if (typeof self !== 'undefined') self.globalThis = self;
        else if (typeof global !== 'undefined') global.globalThis = global;
    }
    if (!Array.isArray(__UTOOPACK__)) {
        return;
    }
    var CHUNK_BASE_PATH = "";
    var CHUNK_SUFFIX_PATH = "";
    var RELATIVE_ROOT_PATH = "..";
    var RUNTIME_PUBLIC_PATH = "";
    // Library builds deliberately collapse JavaScript into one chunk, so the
    // component-chunk runtime path is unsupported in this custom runtime.
    var SUPPORT_COMPONENT_CHUNKS = false;
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
 */ var SourceType = function(SourceType) {
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
 */ var createModuleWithDirectionFlag = false;
    var REEXPORTED_OBJECTS = new WeakMap();
    /**
 * Constructs the `__turbopack_context__` object for a module.
 */ function Context(module1, exports1) {
        this.m = module1;
        // We need to store this here instead of accessing it from the module object to:
        // 1. Make it available to factories directly, since we rewrite `this` to
        //    `__turbopack_context__.e` in CJS modules.
        // 2. Support async modules which rewrite `module.exports` to a promise, so we
        //    can still access the original exports object from functions like
        //    `esmExport`
        // Ideally we could find a new approach for async modules and drop this property altogether.
        this.e = exports1;
    }
    var contextPrototype = Context.prototype;
    var hasOwnProperty = Object.prototype.hasOwnProperty;
    var toStringTag = typeof Symbol !== 'undefined' && Symbol.toStringTag;
    function defineProp(obj, name, options) {
        if (!hasOwnProperty.call(obj, name)) Object.defineProperty(obj, name, options);
    }
    function getOverwrittenModule(moduleCache, id) {
        var _$module = moduleCache[id];
        if (!_$module) {
            if (createModuleWithDirectionFlag) {
                // set in development modes for hmr support
                _$module = createModuleWithDirection(id);
            } else {
                _$module = createModuleObject(id);
            }
            moduleCache[id] = _$module;
        }
        return _$module;
    }
    /**
 * Creates the module object. Only done here to ensure all module objects have the same shape.
 */ function createModuleObject(id) {
        return {
            exports: {},
            error: undefined,
            id: id,
            namespaceObject: undefined
        };
    }
    function createModuleWithDirection(id) {
        return {
            exports: {},
            error: undefined,
            id: id,
            namespaceObject: undefined,
            parents: [],
            children: []
        };
    }
    var BindingTag_Value = 0;
    /**
 * Adds the getters to the exports object.
 */ function esm(exports1, bindings, dynamic) {
        defineProp(exports1, '__esModule', {
            value: true
        });
        if (toStringTag) defineProp(exports1, toStringTag, {
            value: 'Module'
        });
        var i = 0;
        while(i < bindings.length){
            var propName = bindings[i++];
            var tagOrFunction = bindings[i++];
            if (typeof tagOrFunction === 'number') {
                if (tagOrFunction === BindingTag_Value) {
                    defineProp(exports1, propName, {
                        value: bindings[i++],
                        enumerable: true,
                        writable: false
                    });
                } else {
                    throw new Error("unexpected tag: ".concat(tagOrFunction));
                }
            } else {
                var getterFn = tagOrFunction;
                if (typeof bindings[i] === 'function') {
                    var setterFn = bindings[i++];
                    defineProp(exports1, propName, {
                        get: getterFn,
                        set: setterFn,
                        enumerable: true
                    });
                } else {
                    defineProp(exports1, propName, {
                        get: getterFn,
                        enumerable: true
                    });
                }
            }
        }
        // The properties defined above are already non-configurable and
        // non-writable, so the namespace's existing exports are effectively
        // immutable. Sealing additionally makes the object non-extensible, matching
        // real ESM-namespace semantics. Modules with dynamic re-exports
        // (`export *` from a CommonJS module) must stay extensible so the dynamic
        // export proxy can surface keys discovered at runtime, so skip the seal for
        // them.
        if (!dynamic) Object.seal(exports1);
    }
    /**
 * Makes the module an ESM with exports
 */ function esmExport(bindings, id, dynamic) {
        var _$module;
        var _$exports;
        if (id != null) {
            _$module = getOverwrittenModule(this.c, id);
            _$exports = _$module.exports;
        } else {
            _$module = this.m;
            _$exports = this.e;
        }
        _$module.namespaceObject = _$exports;
        esm(_$exports, bindings, dynamic);
    }
    contextPrototype.s = esmExport;
    function ensureDynamicExports(module1, exports1) {
        var reexportedObjects = REEXPORTED_OBJECTS.get(module1);
        if (!reexportedObjects) {
            REEXPORTED_OBJECTS.set(module1, reexportedObjects = []);
            // Returns the re-exported object that provides `prop` as an own property,
            // or `undefined` if none does. The traps share this logic so they always
            // agree on which keys are synthesized from `reexportedObjects`. `default`
            // is never re-exported by `export *`, so it is never synthesized.
            var reexportOwning = function reexportOwning(prop) {
                if (prop !== 'default') {
                    var _iteratorNormalCompletion = true, _didIteratorError = false, _iteratorError = undefined;
                    try {
                        for(var _iterator = reexportedObjects[Symbol.iterator](), _step; !(_iteratorNormalCompletion = (_step = _iterator.next()).done); _iteratorNormalCompletion = true){
                            var obj = _step.value;
                            if (hasOwnProperty.call(obj, prop)) return obj;
                        }
                    } catch (err) {
                        _didIteratorError = true;
                        _iteratorError = err;
                    } finally{
                        try {
                            if (!_iteratorNormalCompletion && _iterator.return != null) {
                                _iterator.return();
                            }
                        } finally{
                            if (_didIteratorError) {
                                throw _iteratorError;
                            }
                        }
                    }
                }
                return undefined;
            };
            // Modules with dynamic re-exports are not sealed by `esm()`, so the
            // target beneath the namespace stays extensible. That is what lets the
            // `ownKeys` and `getOwnPropertyDescriptor` traps legally report keys that
            // exist on `reexportedObjects` but not on the target itself.
            module1.exports = module1.namespaceObject = new Proxy(exports1, {
                get: function get(target, prop) {
                    if (hasOwnProperty.call(target, prop) || prop === 'default' || prop === '__esModule') {
                        return Reflect.get(target, prop);
                    }
                    var obj = reexportOwning(prop);
                    return obj && Reflect.get(obj, prop);
                },
                // The namespace is read-only, like a real esm namespace object. The
                // re-exported modules can still mutate their own exports (exposed live
                // via `get`), but mutating the namespace itself is rejected. Refusing
                // here, rather than forwarding to the extensible target, also prevents an
                // assignment/definition from shadowing a dynamic re-export. It also
                // prevents delete from removing a static export.
                set: function set() {
                    return false;
                },
                defineProperty: function defineProperty() {
                    return false;
                },
                deleteProperty: function deleteProperty() {
                    return false;
                },
                // The `has` trap ensures that `'exportName' in starImports` will reflect
                // the truth of whether a key is exported.
                has: function has(target, prop) {
                    if (Reflect.has(target, prop)) return true;
                    if (prop === 'default' || prop === '__esModule') return false;
                    return reexportOwning(prop) !== undefined;
                },
                // ownKeys and getOwnPropertyDescriptor together make the keys enumerable.
                // If a value is returned from `ownKeys` but its property descriptor is
                // not enumerable, it will not be visible to iterator methods.
                // Collectively, they allow code like the following:
                //
                // ```
                // // module.js re-exports dynamic CJS exports
                // export * from './legacyModule.cjs'
                //
                // // from another JS file, reference the re-exported dynamic values
                // import * as Namespace from './module.js'
                // Object.keys(Namespace)
                // ```
                ownKeys: function ownKeys(target) {
                    var keys = Reflect.ownKeys(target);
                    var _iteratorNormalCompletion = true, _didIteratorError = false, _iteratorError = undefined;
                    try {
                        for(var _iterator = reexportedObjects[Symbol.iterator](), _step; !(_iteratorNormalCompletion = (_step = _iterator.next()).done); _iteratorNormalCompletion = true){
                            var obj = _step.value;
                            var _iteratorNormalCompletion1 = true, _didIteratorError1 = false, _iteratorError1 = undefined;
                            try {
                                for(var _iterator1 = Reflect.ownKeys(obj)[Symbol.iterator](), _step1; !(_iteratorNormalCompletion1 = (_step1 = _iterator1.next()).done); _iteratorNormalCompletion1 = true){
                                    var key = _step1.value;
                                    if (key !== 'default' && !keys.includes(key)) keys.push(key);
                                }
                            } catch (err) {
                                _didIteratorError1 = true;
                                _iteratorError1 = err;
                            } finally{
                                try {
                                    if (!_iteratorNormalCompletion1 && _iterator1.return != null) {
                                        _iterator1.return();
                                    }
                                } finally{
                                    if (_didIteratorError1) {
                                        throw _iteratorError1;
                                    }
                                }
                            }
                        }
                    } catch (err) {
                        _didIteratorError = true;
                        _iteratorError = err;
                    } finally{
                        try {
                            if (!_iteratorNormalCompletion && _iterator.return != null) {
                                _iterator.return();
                            }
                        } finally{
                            if (_didIteratorError) {
                                throw _iteratorError;
                            }
                        }
                    }
                    return keys;
                },
                getOwnPropertyDescriptor: function getOwnPropertyDescriptor(target, prop) {
                    var own = Reflect.getOwnPropertyDescriptor(target, prop);
                    if (own || prop === 'default' || prop === '__esModule') return own;
                    var obj = reexportOwning(prop);
                    if (obj) {
                        // Synthetic keys don't exist on the target, so they MUST be
                        // reported as configurable. However the set/delete traps above will
                        // prevent them from actually being changed
                        return {
                            enumerable: true,
                            configurable: true,
                            get: function get() {
                                return Reflect.get(obj, prop);
                            }
                        };
                    }
                    return undefined;
                }
            });
        }
        return reexportedObjects;
    }
    /**
 * Dynamically exports properties from an object
 */ function dynamicExport(object, id) {
        var _$module;
        var _$exports;
        if (id != null) {
            _$module = getOverwrittenModule(this.c, id);
            _$exports = _$module.exports;
        } else {
            _$module = this.m;
            _$exports = this.e;
        }
        var reexportedObjects = ensureDynamicExports(_$module, _$exports);
        if ((typeof object === "undefined" ? "undefined" : _type_of(object)) === 'object' && object !== null) {
            reexportedObjects.push(object);
        }
    }
    contextPrototype.j = dynamicExport;
    function exportValue(value, id) {
        var _$module;
        if (id != null) {
            _$module = getOverwrittenModule(this.c, id);
        } else {
            _$module = this.m;
        }
        _$module.exports = value;
    }
    contextPrototype.v = exportValue;
    function exportNamespace(namespace, id) {
        var _$module;
        if (id != null) {
            _$module = getOverwrittenModule(this.c, id);
        } else {
            _$module = this.m;
        }
        _$module.exports = _$module.namespaceObject = namespace;
    }
    contextPrototype.n = exportNamespace;
    function createGetter(obj, key) {
        return function() {
            return obj[key];
        };
    }
    /**
 * @returns prototype of the object
 */ var getProto = Object.getPrototypeOf ? function(obj) {
        return Object.getPrototypeOf(obj);
    } : function(obj) {
        return obj.__proto__;
    };
    /** Prototypes that are not expanded for exports */ var LEAF_PROTOTYPES = [
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
        var bindings = [];
        var defaultLocation = -1;
        for(var current = raw; ((typeof current === "undefined" ? "undefined" : _type_of(current)) === 'object' || typeof current === 'function') && !LEAF_PROTOTYPES.includes(current); current = getProto(current)){
            var _iteratorNormalCompletion = true, _didIteratorError = false, _iteratorError = undefined;
            try {
                for(var _iterator = Object.getOwnPropertyNames(current)[Symbol.iterator](), _step; !(_iteratorNormalCompletion = (_step = _iterator.next()).done); _iteratorNormalCompletion = true){
                    var key = _step.value;
                    bindings.push(key, createGetter(raw, key));
                    if (defaultLocation === -1 && key === 'default') {
                        defaultLocation = bindings.length - 1;
                    }
                }
            } catch (err) {
                _didIteratorError = true;
                _iteratorError = err;
            } finally{
                try {
                    if (!_iteratorNormalCompletion && _iterator.return != null) {
                        _iterator.return();
                    }
                } finally{
                    if (_didIteratorError) {
                        throw _iteratorError;
                    }
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
            return function() {
                for(var _len = arguments.length, args = new Array(_len), _key = 0; _key < _len; _key++){
                    args[_key] = arguments[_key];
                }
                return raw.apply(this, args);
            };
        } else {
            return Object.create(null);
        }
    }
    function esmImport(id) {
        var _$module = getOrInstantiateModuleFromParent(id, this.m);
        // any ES module has to have `module.namespaceObject` defined.
        if (_$module.namespaceObject) return _$module.namespaceObject;
        // only ESM can be an async module, so we don't need to worry about exports being a promise here.
        var raw = _$module.exports;
        return _$module.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
    }
    contextPrototype.i = esmImport;
    function asyncLoader(moduleId) {
        var loader = this.r(moduleId);
        return loader(esmImport.bind(this));
    }
    contextPrototype.A = asyncLoader;
    // Add a simple runtime require so that environments without one can still pass
    // `typeof require` CommonJS checks so that exports are correctly registered.
    var runtimeRequire = typeof require === 'function' ? require : function require1() {
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
        var hashIndex = request.indexOf('#');
        if (hashIndex !== -1) {
            request = request.substring(0, hashIndex);
        }
        var queryIndex = request.indexOf('?');
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
            var e = new Error("Cannot find module '".concat(id, "'"));
            e.code = 'MODULE_NOT_FOUND';
            throw e;
        }
        moduleContext.keys = function() {
            return Object.keys(map);
        };
        moduleContext.resolve = function(id) {
            id = parseRequest(id);
            if (hasOwnProperty.call(map, id)) {
                return map[id].id();
            }
            var e = new Error("Cannot find module '".concat(id, "'"));
            e.code = 'MODULE_NOT_FOUND';
            throw e;
        };
        moduleContext.import = function(id) {
            return _async_to_generator(function() {
                return _ts_generator(this, function(_state) {
                    switch(_state.label){
                        case 0:
                            return [
                                4,
                                moduleContext(id)
                            ];
                        case 1:
                            return [
                                2,
                                _state.sent()
                            ];
                    }
                });
            })();
        };
        return moduleContext;
    }
    contextPrototype.f = moduleContext;
    /**
 * Returns the path of a chunk defined by its data.
 */ function getChunkPath(chunkData) {
        return typeof chunkData === 'string' ? chunkData : chunkData.path;
    }
    // Load the CompressedModuleFactories of a chunk into the `moduleFactories` Map.
    // The flat format alternates one or more module IDs with their factory function.
    // Strict factories can be prepended as a nested array.
    function installCompressedModuleFactories(chunkModules, offset, moduleFactories, newModuleId) {
        var i = offset;
        var strictFactories = chunkModules[i];
        if (Array.isArray(strictFactories)) {
            installCompressedModuleFactories(strictFactories, 0, moduleFactories, newModuleId);
            i++;
        }
        while(i < chunkModules.length){
            var end = i + 1;
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
            var moduleFactoryFn = chunkModules[end];
            var existingGroupFactory = undefined;
            for(var j = i; j < end; j++){
                var id = chunkModules[j];
                var existingFactory = moduleFactories.get(id);
                if (existingFactory) {
                    existingGroupFactory = existingFactory;
                    break;
                }
            }
            var factoryToInstall = existingGroupFactory !== null && existingGroupFactory !== void 0 ? existingGroupFactory : moduleFactoryFn;
            var didInstallFactory = false;
            for(var j1 = i; j1 < end; j1++){
                var id1 = chunkModules[j1];
                if (!moduleFactories.has(id1)) {
                    if (!didInstallFactory) {
                        if (factoryToInstall === moduleFactoryFn) {
                            applyModuleFactoryName(moduleFactoryFn);
                        }
                        didInstallFactory = true;
                    }
                    moduleFactories.set(id1, factoryToInstall);
                    newModuleId === null || newModuleId === void 0 ? void 0 : newModuleId(id1);
                }
            }
            i = end + 1;
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
 */ var relativeURL = function relativeURL(inputUrl) {
        var realUrl = new URL(inputUrl, 'x:/');
        var values = {};
        for(var key in realUrl)values[key] = realUrl[key];
        values.href = inputUrl;
        values.pathname = inputUrl.replace(/[?#].*/, '');
        values.origin = values.protocol = '';
        values.toString = values.toJSON = function() {
            for(var _len = arguments.length, _args = new Array(_len), _key = 0; _key < _len; _key++){
                _args[_key] = arguments[_key];
            }
            return inputUrl;
        };
        for(var key1 in values)Object.defineProperty(this, key1, {
            enumerable: true,
            configurable: true,
            value: values[key1]
        });
    };
    relativeURL.prototype = URL.prototype;
    contextPrototype.U = relativeURL;
    /**
 * Utility function to ensure all variants of an enum are handled.
 */ function invariant(never, computeMessage) {
        throw new Error("Invariant: ".concat(computeMessage(never)));
    }
    /**
 * Constructs an error message for when a module factory is not available.
 */ function factoryNotAvailableMessage(moduleId, sourceType, sourceData) {
        var instantiationReason;
        switch(sourceType){
            case 0:
                instantiationReason = "as a runtime entry of chunk ".concat(sourceData);
                break;
            case 1:
                instantiationReason = "because it was required from module ".concat(sourceData);
                break;
            case 2:
                instantiationReason = 'because of an HMR update';
                break;
            default:
                invariant(sourceType, function(sourceType) {
                    return "Unknown source type: ".concat(sourceType);
                });
        }
        return "Module ".concat(moduleId, " was instantiated ").concat(instantiationReason, ", but the module factory is not available.");
    }
    /**
 * A stub function to make `require` available but non-functional in ESM.
 */ function requireStub(_moduleId) {
        throw new Error('dynamic usage of require is not supported');
    }
    contextPrototype.z = requireStub;
    // Make `globalThis` available to the module in a way that cannot be shadowed by a local variable.
    contextPrototype.g = globalThis;
    var cachedAutomaticPublicPath;
    function getAutomaticPublicPath() {
        if (cachedAutomaticPublicPath !== undefined) {
            return cachedAutomaticPublicPath;
        }
        var scriptUrl;
        if ((typeof document === "undefined" ? "undefined" : _type_of(document)) === 'object') {
            var currentScript = document.currentScript;
            scriptUrl = currentScript === null || currentScript === void 0 ? void 0 : currentScript.src;
            if (!scriptUrl) {
                var scripts = document.getElementsByTagName('script');
                var script = scripts[scripts.length - 1];
                scriptUrl = script === null || script === void 0 ? void 0 : script.src;
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
            var publicPath = globalThis.publicPath;
            return publicPath.endsWith('/') ? publicPath : "".concat(publicPath, "/");
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
    /// <reference path="./runtime-types.d.ts" />
    /// <reference path="./runtime-utils.ts" />
    /**
 * Top-level-await / async-module machinery. This is only included in the runtime
 * when the module graph actually contains an async module (a module with
 * top-level await, or one that transitively depends on one). When no async
 * module is present, the chunk items never reference `__turbopack_context__.a`,
 * so this whole file can be omitted.
 *
 * everything below is adapted from webpack
 * https://github.com/webpack/webpack/blob/6be4065ade1e252c1d8dcba4af0f43e32af1bdc1/lib/runtime/AsyncModuleRuntimeModule.js#L13
 */ var turbopackQueues = Symbol('turbopack queues');
    var turbopackExports = Symbol('turbopack exports');
    var turbopackError = Symbol('turbopack error');
    function isPromise(maybePromise) {
        return maybePromise != null && (typeof maybePromise === "undefined" ? "undefined" : _type_of(maybePromise)) === 'object' && 'then' in maybePromise && typeof maybePromise.then === 'function';
    }
    function isAsyncModuleExt(obj) {
        return turbopackQueues in obj;
    }
    function createPromise() {
        var resolve;
        var reject;
        var promise = new Promise(function(res, rej) {
            reject = rej;
            resolve = res;
        });
        return {
            promise: promise,
            resolve: resolve,
            reject: reject
        };
    }
    function resolveQueue(queue) {
        if (queue && queue.status !== 1) {
            queue.status = 1;
            queue.forEach(function(fn) {
                return fn.queueCount--;
            });
            queue.forEach(function(fn) {
                return fn.queueCount-- ? fn.queueCount++ : fn();
            });
        }
    }
    function wrapDeps(deps) {
        return deps.map(function(dep) {
            if (dep !== null && (typeof dep === "undefined" ? "undefined" : _type_of(dep)) === 'object') {
                if (isAsyncModuleExt(dep)) return dep;
                if (isPromise(dep)) {
                    var queue = Object.assign([], {
                        status: 0
                    });
                    var _obj;
                    var obj = (_obj = {}, _define_property(_obj, turbopackExports, {}), _define_property(_obj, turbopackQueues, function(fn) {
                        return fn(queue);
                    }), _obj);
                    dep.then(function(res) {
                        obj[turbopackExports] = res;
                        resolveQueue(queue);
                    }, function(err) {
                        obj[turbopackError] = err;
                        resolveQueue(queue);
                    });
                    return obj;
                }
            }
            var _obj1;
            return _obj1 = {}, _define_property(_obj1, turbopackExports, dep), _define_property(_obj1, turbopackQueues, function() {}), _obj1;
        });
    }
    function asyncModule(body, hasAwait) {
        var _$module = this.m;
        var queue = hasAwait ? Object.assign([], {
            status: -1
        }) : undefined;
        var depQueues = new Set();
        var _createPromise = createPromise(), resolve = _createPromise.resolve, reject = _createPromise.reject, rawPromise = _createPromise.promise;
        var _obj;
        var promise = Object.assign(rawPromise, (_obj = {}, _define_property(_obj, turbopackExports, _$module.exports), _define_property(_obj, turbopackQueues, function(fn) {
            queue && fn(queue);
            depQueues.forEach(fn);
            promise['catch'](function() {});
        }), _obj));
        var attributes = {
            get: function get() {
                return promise;
            },
            set: function set(v) {
                // Calling `esmExport` leads to this.
                if (v !== promise) {
                    promise[turbopackExports] = v;
                }
            }
        };
        Object.defineProperty(_$module, 'exports', attributes);
        Object.defineProperty(_$module, 'namespaceObject', attributes);
        function handleAsyncDependencies(deps) {
            var currentDeps = wrapDeps(deps);
            var getResult = function getResult() {
                return currentDeps.map(function(d) {
                    if (d[turbopackError]) throw d[turbopackError];
                    return d[turbopackExports];
                });
            };
            var _createPromise = createPromise(), promise = _createPromise.promise, resolve = _createPromise.resolve;
            var fn = Object.assign(function() {
                return resolve(getResult);
            }, {
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
            currentDeps.map(function(dep) {
                return dep[turbopackQueues](fnQueue);
            });
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
 * This file contains runtime types and functions that are shared between all
 * Turbopack UMD library runtimes (DOM and Node.js).
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */ /* eslint-disable @typescript-eslint/no-unused-vars */ /// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-utils.ts" />
    /// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-types.d.ts" />
    // Provided by build
    var BACKEND;
    var moduleFactories = new Map();
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
 */ function externalRequire(id, thunk) {
        var esm = arguments.length > 2 && arguments[2] !== void 0 ? arguments[2] : false;
        var raw;
        try {
            raw = thunk();
        } catch (err) {
            throw new Error("Failed to load external module ".concat(id, ": ").concat(err));
        }
        if (!esm || raw.__esModule) {
            return raw;
        }
        return interopEsm(raw, createNS(raw), true);
    }
    externalRequire.resolve = function(id, options) {
        return require.resolve(id, options);
    };
    contextPrototype.x = externalRequire;
    /**
 * Adds Webpack-compatible ESM metadata to external values while preserving
 * native ESM live bindings.
 */ function externalNamespace(mod) {
        if (mod && mod.__esModule) return mod;
        var ns = Object.create(null);
        var isEsmNamespace = mod && toStringTag && mod[toStringTag] === "Module";
        if (mod && ((typeof mod === "undefined" ? "undefined" : _type_of(mod)) === "object" || typeof mod === "function")) {
            for(var key in mod){
                if (key === "__esModule" || !isEsmNamespace && key === "default") {
                    continue;
                }
                Object.defineProperty(ns, key, {
                    enumerable: true,
                    get: createGetter(mod, key)
                });
            }
        }
        if (!isEsmNamespace) {
            Object.defineProperty(ns, "default", {
                enumerable: true,
                value: mod
            });
        }
        Object.defineProperty(ns, "__esModule", {
            value: true
        });
        if (toStringTag) {
            Object.defineProperty(ns, toStringTag, {
                value: "Module"
            });
        }
        return ns;
    }
    contextPrototype.N = externalNamespace;
    /// <reference path="./runtime-base.ts" />
    /// <reference path="./dummy.ts" />
    var moduleCache = {};
    contextPrototype.c = moduleCache;
    /**
 * Gets or instantiates a runtime module.
 */ // @ts-ignore
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    function getOrInstantiateRuntimeModule(chunkPath, moduleId) {
        var _$module = moduleCache[moduleId];
        if (_$module) {
            if (_$module.error) {
                throw _$module.error;
            }
            return _$module;
        }
        return instantiateModule(moduleId, SourceType.Runtime, chunkPath);
    }
    /**
 * Retrieves a module from the cache, or instantiate it if it is not cached.
 */ // Used by the backend
    // @ts-ignore
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    var getOrInstantiateModuleFromParent = function getOrInstantiateModuleFromParent(id, sourceModule) {
        var _$module = moduleCache[id];
        if (_$module) {
            if (_$module.error) {
                throw _$module.error;
            }
            return _$module;
        }
        return instantiateModule(id, SourceType.Parent, sourceModule.id);
    };
    function instantiateModule(id, sourceType, sourceData) {
        var moduleFactory = moduleFactories.get(id);
        if (typeof moduleFactory !== 'function') {
            // This can happen if modules incorrectly handle HMR disposes/updates,
            // e.g. when they keep a `setTimeout` around which still executes old code
            // and contains e.g. a `require("something")` call.
            throw new Error(factoryNotAvailableMessage(id, sourceType, sourceData));
        }
        var _$module = createModuleObject(id);
        var _$exports = _$module.exports;
        moduleCache[id] = _$module;
        // NOTE(alexkirsz) This can fail when the module encounters a runtime error.
        var context = new Context(_$module, _$exports);
        try {
            moduleFactory(context, _$module, _$exports);
        } catch (error) {
            _$module.error = error;
            throw error;
        }
        if (_$module.namespaceObject && _$module.exports !== _$module.namespaceObject) {
            // in case of a circular dependency: cjs1 -> esm2 -> cjs1
            interopEsm(_$module.exports, _$module.namespaceObject);
        }
        return _$module;
    }
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    function registerChunk(registration) {
        // An inlined entry-only registration is a bare params object (no source chunk).
        if (!Array.isArray(registration)) {
            return BACKEND.registerChunk(undefined, registration);
        }
        var chunk = getChunkFromRegistration(registration[0]);
        if (SUPPORT_COMPONENT_CHUNKS) {
            markChunkComponentsAvailable(chunk);
        }
        var runtimeParams;
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
    var loadedScripts = new Map();
    /**
 * Load an external script by creating a <script> tag.
 * This is used for script externals that need to be loaded from CDN or other external sources.
 */ function loadScript(scriptUrl) {
        // Return cached promise if script is already loading or loaded
        var promise = loadedScripts.get(scriptUrl);
        if (promise) {
            return promise;
        }
        promise = new Promise(function(resolve, reject) {
            var script = document.createElement("script");
            script.src = scriptUrl;
            script.onload = function() {
                return resolve();
            };
            script.onerror = function() {
                return reject(new Error("Failed to load script: ".concat(scriptUrl)));
            };
            document.head.appendChild(script);
        });
        loadedScripts.set(scriptUrl, promise);
        return promise;
    }
    contextPrototype.S = loadScript;
    (function() {
        BACKEND = {
            registerChunk: function registerChunk(chunk, params) {
                if (params == null) {
                    return;
                }
                if (params.runtimeModuleIds.length > 0) {
                    var _iteratorNormalCompletion = true, _didIteratorError = false, _iteratorError = undefined;
                    try {
                        for(var _iterator = params.runtimeModuleIds[Symbol.iterator](), _step; !(_iteratorNormalCompletion = (_step = _iterator.next()).done); _iteratorNormalCompletion = true){
                            var moduleId = _step.value;
                            getOrInstantiateRuntimeModule(chunk, moduleId);
                        }
                    } catch (err) {
                        _didIteratorError = true;
                        _iteratorError = err;
                    } finally{
                        try {
                            if (!_iteratorNormalCompletion && _iterator.return != null) {
                                _iterator.return();
                            }
                        } finally{
                            if (_didIteratorError) {
                                throw _iteratorError;
                            }
                        }
                    }
                }
            }
        };
    })();
    var chunksToRegister = __UTOOPACK__;
    __UTOOPACK__ = {
        push: registerChunk
    };
    chunksToRegister.forEach(registerChunk);
    function factory() {
        var runtimeModuleIds = [
            "[project]/runtime/library_build_runtime_legacy/input/index.js [library-client] (ecmascript)"
        ];
        var _$exports;
        for(var i = 0; i < runtimeModuleIds.length; i++){
            var _$module = moduleCache[runtimeModuleIds[i]];
            if (_$module.error) throw _$module.error;
            _$exports = _$module;
        }
        if (_$exports) {
            // any ES module has to have `module.namespaceObject` defined.
            if (_$exports.namespaceObject) return _$exports.namespaceObject;
            // only ESM can be an async module, so we don't need to worry about exports being a promise here.
            var raw = _$exports.exports;
            return _$exports.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
        }
    }
    if ((typeof exports === "undefined" ? "undefined" : _type_of(exports)) === 'object' && (typeof module === "undefined" ? "undefined" : _type_of(module)) === 'object') {
        module.exports = factory();
    } else if ((typeof exports === "undefined" ? "undefined" : _type_of(exports)) === 'object') {
        exports["LegacyLibrary"] = factory();
    } else {
        globalThis["LegacyLibrary"] = factory();
    }
})([
    [
        "main.js",
        "[project]/runtime/library_build_runtime_legacy/input/asset.svg (static in ecmascript)",
        function(__turbopack_context__) {
            __turbopack_context__.v(__turbopack_context__.p("auto") + "asset.36cae746.svg");
        },
        "[project]/runtime/library_build_runtime_legacy/input/index.js [library-client] (ecmascript)",
        function(__turbopack_context__) {
            "use strict";
            var __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$index$2e$js__$5b$library$2d$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__ = __turbopack_context__.i("[project]/runtime/library_build_runtime_legacy/input/index.js [library-client] (ecmascript) <locals>");
            var __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$asset$2e$svg__$28$static__in__ecmascript$29$__ = __turbopack_context__.i("[project]/runtime/library_build_runtime_legacy/input/asset.svg (static in ecmascript)");
            __turbopack_context__.s([
                "asset",
                function() {
                    return __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$asset$2e$svg__$28$static__in__ecmascript$29$__["default"];
                },
                "flag",
                function() {
                    return __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$index$2e$js__$5b$library$2d$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__["flag"];
                },
                "last",
                function() {
                    return __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$index$2e$js__$5b$library$2d$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__["last"];
                },
                "load",
                function() {
                    return __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$index$2e$js__$5b$library$2d$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__["load"];
                },
                "read",
                function() {
                    return __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$index$2e$js__$5b$library$2d$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__["read"];
                }
            ]);
        },
        "[project]/runtime/library_build_runtime_legacy/input/index.js [library-client] (ecmascript) <locals>",
        function(__turbopack_context__) {
            "use strict";
            var __TURBOPACK__imported__module__$5b$project$5d2f$runtime$2f$library_build_runtime_legacy$2f$input$2f$asset$2e$svg__$28$static__in__ecmascript$29$__ = __turbopack_context__.i("[project]/runtime/library_build_runtime_legacy/input/asset.svg (static in ecmascript)");
            ;
            ;
            var read = function read(value) {
                var _ref;
                return (_ref = value === null || value === void 0 ? void 0 : value.answer) !== null && _ref !== void 0 ? _ref : 42;
            };
            var flag = 1;
            var load = function load() {
                return Promise.resolve().then(function() {
                    return __turbopack_context__.i("[project]/runtime/library_build_runtime_legacy/input/lazy.js [library-client] (ecmascript)");
                }).then(function(module1) {
                    return module1.answer;
                });
            };
            function last(node) {
                do {
                    node = node.next;
                }while (node.next);
                return node.value;
            }
            __turbopack_context__.s([
                "flag",
                0,
                flag,
                "last",
                0,
                last,
                "load",
                0,
                load,
                "read",
                0,
                read
            ]);
        },
        "[project]/runtime/library_build_runtime_legacy/input/lazy.js [library-client] (ecmascript)",
        function(__turbopack_context__) {
            "use strict";
            return __turbopack_context__.a(function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
                var __gen = function() {
                    var answer, e;
                    return _ts_generator(this, function(_state) {
                        switch(_state.label){
                            case 0:
                                _state.trys.push([
                                    0,
                                    2,
                                    ,
                                    3
                                ]);
                                return [
                                    4,
                                    Promise.resolve(43)
                                ];
                            case 1:
                                answer = _state.sent();
                                __turbopack_context__.s([
                                    "answer",
                                    0,
                                    answer
                                ]);
                                __turbopack_async_result__();
                                return [
                                    3,
                                    3
                                ];
                            case 2:
                                e = _state.sent();
                                __turbopack_async_result__(e);
                                return [
                                    3,
                                    3
                                ];
                            case 3:
                                return [
                                    2
                                ];
                        }
                    });
                }();
                (function __step(k, a) {
                    try {
                        var r = __gen[k](a);
                    } catch (e) {
                        __turbopack_async_result__(e);
                        return;
                    }
                    if (!r.done) Promise.resolve(r.value).then(function(v) {
                        __step('next', v);
                    }, function(e) {
                        __step('throw', e);
                    });
                })('next');
            }, true);
        }
    ],
    [
        "main.js",
        {
            "otherChunks": [],
            "runtimeModuleIds": [
                "[project]/runtime/library_build_runtime_legacy/input/index.js [library-client] (ecmascript)"
            ]
        }
    ]
]);

}).call(this);