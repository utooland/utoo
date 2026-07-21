(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[@utoo/pack-runtime]/react-refresh/internal/helpers.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "default",
    ()=>__TURBOPACK__default__export__
]);
/**
 * MIT License
 *
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */ // This file is copied from the Metro JavaScript bundler, with minor tweaks for
// webpack 4 compatibility.
//
// https://github.com/facebook/metro/blob/d6b9685c730d0d63577db40f41369157f28dfa3a/packages/metro/src/lib/polyfills/require.js
var __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[@utoo/pack-runtime]/react-refresh/internal/react-refresh-runtime.development.js [client] (ecmascript)");
;
const { m: turbopackModule } = __turbopack_context__;
function isSafeExport(key) {
    return key === "__esModule" || key === "__N_SSG" || key === "__N_SSP" || // TODO: remove this key from page config instead of allow listing it
    key === "config";
}
function registerExportsForReactRefresh(moduleExports, moduleID) {
    __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].register(moduleExports, moduleID + " %exports%");
    if (moduleExports == null || typeof moduleExports !== "object") {
        // Exit if we can't iterate over exports.
        // (This is important for legacy environments.)
        return;
    }
    for(var key in moduleExports){
        if (isSafeExport(key)) {
            continue;
        }
        try {
            var exportValue = moduleExports[key];
        } catch  {
            continue;
        }
        var typeID = moduleID + " %exports% " + key;
        __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].register(exportValue, typeID);
    }
}
function getRefreshBoundarySignature(moduleExports) {
    var signature = [];
    signature.push(__TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].getFamilyByType(moduleExports));
    if (moduleExports == null || typeof moduleExports !== "object") {
        // Exit if we can't iterate over exports.
        // (This is important for legacy environments.)
        return signature;
    }
    for(var key in moduleExports){
        if (isSafeExport(key)) {
            continue;
        }
        try {
            var exportValue = moduleExports[key];
        } catch  {
            continue;
        }
        signature.push(key);
        signature.push(__TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].getFamilyByType(exportValue));
    }
    return signature;
}
function isReactRefreshBoundary(moduleExports) {
    if (__TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].isLikelyComponentType(moduleExports)) {
        return true;
    }
    if (moduleExports == null || typeof moduleExports !== "object") {
        // Exit if we can't iterate over exports.
        return false;
    }
    var hasExports = false;
    var areAllExportsComponents = true;
    for(var key in moduleExports){
        hasExports = true;
        if (isSafeExport(key)) {
            continue;
        }
        try {
            var exportValue = moduleExports[key];
        } catch  {
            // This might fail due to circular dependencies
            return false;
        }
        if (!__TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].isLikelyComponentType(exportValue)) {
            areAllExportsComponents = false;
        }
    }
    return hasExports && areAllExportsComponents;
}
function shouldInvalidateReactRefreshBoundary(prevSignature, nextSignature) {
    if (prevSignature.length !== nextSignature.length) {
        return true;
    }
    for(var i = 0; i < nextSignature.length; i++){
        if (prevSignature[i] !== nextSignature[i]) {
            return true;
        }
    }
    return false;
}
var isUpdateScheduled = false;
// This function aggregates updates from multiple modules into a single React Refresh call.
function scheduleUpdate() {
    if (isUpdateScheduled) {
        return;
    }
    isUpdateScheduled = true;
    function canApplyUpdate(status) {
        return status === "idle";
    }
    function applyUpdate() {
        isUpdateScheduled = false;
        try {
            __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].performReactRefresh();
        } catch (err) {
            console.warn("Warning: Failed to re-render. We will retry on the next Fast Refresh event.\n" + err);
        }
    }
    if (canApplyUpdate(turbopackModule.hot.status())) {
        // Apply update on the next tick.
        Promise.resolve().then(()=>{
            applyUpdate();
        });
        return;
    }
    const statusHandler = (status)=>{
        if (canApplyUpdate(status)) {
            turbopackModule.hot.removeStatusHandler(statusHandler);
            applyUpdate();
        }
    };
    // Apply update once the HMR runtime's status is idle.
    turbopackModule.hot.addStatusHandler(statusHandler);
}
var __TURBOPACK__default__export__ = {
    registerExportsForReactRefresh: registerExportsForReactRefresh,
    isReactRefreshBoundary: isReactRefreshBoundary,
    shouldInvalidateReactRefreshBoundary: shouldInvalidateReactRefreshBoundary,
    getRefreshBoundarySignature: getRefreshBoundarySignature,
    scheduleUpdate: scheduleUpdate
};
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[@utoo/pack-runtime]/react-refresh/internal/react-refresh-runtime.development.js [client] (ecmascript)", ((__turbopack_context__, module, exports) => {
"use strict";

// react@0.18.0: https://unpkg.com/react-refresh@0.18.0/cjs/react-refresh-runtime.development.js
/**
 * @license React
 * react-refresh-runtime.development.js
 *
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */ "production" !== ("TURBOPACK compile-time value", "development") && function() {
    function computeFullKey(signature) {
        if (null !== signature.fullKey) return signature.fullKey;
        var fullKey = signature.ownKey;
        try {
            var hooks = signature.getCustomHooks();
        } catch (err) {
            return signature.forceReset = !0, signature.fullKey = fullKey;
        }
        for(var i = 0; i < hooks.length; i++){
            var hook = hooks[i];
            if ("function" !== typeof hook) return signature.forceReset = !0, signature.fullKey = fullKey;
            hook = allSignaturesByType.get(hook);
            if (void 0 !== hook) {
                var nestedHookKey = computeFullKey(hook);
                hook.forceReset && (signature.forceReset = !0);
                fullKey += "\n---\n" + nestedHookKey;
            }
        }
        return signature.fullKey = fullKey;
    }
    function resolveFamily(type) {
        return updatedFamiliesByType.get(type);
    }
    function cloneMap(map) {
        var clone = new Map();
        map.forEach(function(value, key) {
            clone.set(key, value);
        });
        return clone;
    }
    function cloneSet(set) {
        var clone = new Set();
        set.forEach(function(value) {
            clone.add(value);
        });
        return clone;
    }
    function getProperty(object, property) {
        try {
            return object[property];
        } catch (err) {}
    }
    function register(type, id) {
        if (!(null === type || "function" !== typeof type && "object" !== typeof type || allFamiliesByType.has(type))) {
            var family = allFamiliesByID.get(id);
            void 0 === family ? (family = {
                current: type
            }, allFamiliesByID.set(id, family)) : pendingUpdates.push([
                family,
                type
            ]);
            allFamiliesByType.set(type, family);
            if ("object" === typeof type && null !== type) switch(getProperty(type, "$$typeof")){
                case REACT_FORWARD_REF_TYPE:
                    register(type.render, id + "$render");
                    break;
                case REACT_MEMO_TYPE:
                    register(type.type, id + "$type");
            }
        }
    }
    function setSignature(type, key) {
        var forceReset = 2 < arguments.length && void 0 !== arguments[2] ? arguments[2] : !1, getCustomHooks = 3 < arguments.length ? arguments[3] : void 0;
        allSignaturesByType.has(type) || allSignaturesByType.set(type, {
            forceReset: forceReset,
            ownKey: key,
            fullKey: null,
            getCustomHooks: getCustomHooks || function() {
                return [];
            }
        });
        if ("object" === typeof type && null !== type) switch(getProperty(type, "$$typeof")){
            case REACT_FORWARD_REF_TYPE:
                setSignature(type.render, key, forceReset, getCustomHooks);
                break;
            case REACT_MEMO_TYPE:
                setSignature(type.type, key, forceReset, getCustomHooks);
        }
    }
    function collectCustomHooksForSignature(type) {
        type = allSignaturesByType.get(type);
        void 0 !== type && computeFullKey(type);
    }
    var REACT_FORWARD_REF_TYPE = Symbol.for("react.forward_ref"), REACT_MEMO_TYPE = Symbol.for("react.memo"), PossiblyWeakMap = "function" === typeof WeakMap ? WeakMap : Map, allFamiliesByID = new Map(), allFamiliesByType = new PossiblyWeakMap(), allSignaturesByType = new PossiblyWeakMap(), updatedFamiliesByType = new PossiblyWeakMap(), pendingUpdates = [], helpersByRendererID = new Map(), helpersByRoot = new Map(), mountedRoots = new Set(), failedRoots = new Set(), rootElements = "function" === typeof WeakMap ? new WeakMap() : null, isPerformingRefresh = !1;
    exports._getMountedRootCount = function() {
        return mountedRoots.size;
    };
    exports.collectCustomHooksForSignature = collectCustomHooksForSignature;
    exports.createSignatureFunctionForTransform = function() {
        var savedType, hasCustomHooks, didCollectHooks = !1;
        return function(type, key, forceReset, getCustomHooks) {
            if ("string" === typeof key) return savedType || (savedType = type, hasCustomHooks = "function" === typeof getCustomHooks), null == type || "function" !== typeof type && "object" !== typeof type || setSignature(type, key, forceReset, getCustomHooks), type;
            !didCollectHooks && hasCustomHooks && (didCollectHooks = !0, collectCustomHooksForSignature(savedType));
        };
    };
    exports.getFamilyByID = function(id) {
        return allFamiliesByID.get(id);
    };
    exports.getFamilyByType = function(type) {
        return allFamiliesByType.get(type);
    };
    exports.hasUnrecoverableErrors = function() {
        return !1;
    };
    exports.injectIntoGlobalHook = function(globalObject) {
        var hook = globalObject.__REACT_DEVTOOLS_GLOBAL_HOOK__;
        if (void 0 === hook) {
            var nextID = 0;
            globalObject.__REACT_DEVTOOLS_GLOBAL_HOOK__ = hook = {
                renderers: new Map(),
                supportsFiber: !0,
                inject: function() {
                    return nextID++;
                },
                onScheduleFiberRoot: function() {},
                onCommitFiberRoot: function() {},
                onCommitFiberUnmount: function() {}
            };
        }
        if (hook.isDisabled) console.warn("Something has shimmed the React DevTools global hook (__REACT_DEVTOOLS_GLOBAL_HOOK__). Fast Refresh is not compatible with this shim and will be disabled.");
        else {
            var oldInject = hook.inject;
            hook.inject = function(injected) {
                var id = oldInject.apply(this, arguments);
                "function" === typeof injected.scheduleRefresh && "function" === typeof injected.setRefreshHandler && helpersByRendererID.set(id, injected);
                return id;
            };
            hook.renderers.forEach(function(injected, id) {
                "function" === typeof injected.scheduleRefresh && "function" === typeof injected.setRefreshHandler && helpersByRendererID.set(id, injected);
            });
            var oldOnCommitFiberRoot = hook.onCommitFiberRoot, oldOnScheduleFiberRoot = hook.onScheduleFiberRoot || function() {};
            hook.onScheduleFiberRoot = function(id, root, children) {
                isPerformingRefresh || (failedRoots.delete(root), null !== rootElements && rootElements.set(root, children));
                return oldOnScheduleFiberRoot.apply(this, arguments);
            };
            hook.onCommitFiberRoot = function(id, root, maybePriorityLevel, didError) {
                var helpers = helpersByRendererID.get(id);
                if (void 0 !== helpers) {
                    helpersByRoot.set(root, helpers);
                    helpers = root.current;
                    var alternate = helpers.alternate;
                    null !== alternate ? (alternate = null != alternate.memoizedState && null != alternate.memoizedState.element && mountedRoots.has(root), helpers = null != helpers.memoizedState && null != helpers.memoizedState.element, !alternate && helpers ? (mountedRoots.add(root), failedRoots.delete(root)) : alternate && helpers || (alternate && !helpers ? (mountedRoots.delete(root), didError ? failedRoots.add(root) : helpersByRoot.delete(root)) : alternate || helpers || didError && failedRoots.add(root))) : mountedRoots.add(root);
                }
                return oldOnCommitFiberRoot.apply(this, arguments);
            };
        }
    };
    exports.isLikelyComponentType = function(type) {
        switch(typeof type){
            case "function":
                if (null != type.prototype) {
                    if (type.prototype.isReactComponent) return !0;
                    var ownNames = Object.getOwnPropertyNames(type.prototype);
                    if (1 < ownNames.length || "constructor" !== ownNames[0] || type.prototype.__proto__ !== Object.prototype) return !1;
                }
                type = type.name || type.displayName;
                return "string" === typeof type && /^[A-Z]/.test(type);
            case "object":
                if (null != type) switch(getProperty(type, "$$typeof")){
                    case REACT_FORWARD_REF_TYPE:
                    case REACT_MEMO_TYPE:
                        return !0;
                }
                return !1;
            default:
                return !1;
        }
    };
    exports.performReactRefresh = function() {
        if (0 === pendingUpdates.length || isPerformingRefresh) return null;
        isPerformingRefresh = !0;
        try {
            var staleFamilies = new Set(), updatedFamilies = new Set(), updates = pendingUpdates;
            pendingUpdates = [];
            updates.forEach(function(_ref) {
                var family = _ref[0];
                _ref = _ref[1];
                var prevType = family.current;
                updatedFamiliesByType.set(prevType, family);
                updatedFamiliesByType.set(_ref, family);
                family.current = _ref;
                prevType.prototype && prevType.prototype.isReactComponent || _ref.prototype && _ref.prototype.isReactComponent ? _ref = !1 : (prevType = allSignaturesByType.get(prevType), _ref = allSignaturesByType.get(_ref), _ref = void 0 === prevType && void 0 === _ref || void 0 !== prevType && void 0 !== _ref && computeFullKey(prevType) === computeFullKey(_ref) && !_ref.forceReset ? !0 : !1);
                _ref ? updatedFamilies.add(family) : staleFamilies.add(family);
            });
            var update = {
                updatedFamilies: updatedFamilies,
                staleFamilies: staleFamilies
            };
            helpersByRendererID.forEach(function(helpers) {
                helpers.setRefreshHandler(resolveFamily);
            });
            var didError = !1, firstError = null, failedRootsSnapshot = cloneSet(failedRoots), mountedRootsSnapshot = cloneSet(mountedRoots), helpersByRootSnapshot = cloneMap(helpersByRoot);
            failedRootsSnapshot.forEach(function(root) {
                var helpers = helpersByRootSnapshot.get(root);
                if (void 0 === helpers) throw Error("Could not find helpers for a root. This is a bug in React Refresh.");
                failedRoots.has(root);
                if (null !== rootElements && rootElements.has(root)) {
                    var element = rootElements.get(root);
                    try {
                        helpers.scheduleRoot(root, element);
                    } catch (err) {
                        didError || (didError = !0, firstError = err);
                    }
                }
            });
            mountedRootsSnapshot.forEach(function(root) {
                var helpers = helpersByRootSnapshot.get(root);
                if (void 0 === helpers) throw Error("Could not find helpers for a root. This is a bug in React Refresh.");
                mountedRoots.has(root);
                try {
                    helpers.scheduleRefresh(root, update);
                } catch (err) {
                    didError || (didError = !0, firstError = err);
                }
            });
            if (didError) throw firstError;
            return update;
        } finally{
            isPerformingRefresh = !1;
        }
    };
    exports.register = register;
    exports.setSignature = setSignature;
}();
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[@utoo/pack-runtime]/react-refresh/runtime.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$helpers$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[@utoo/pack-runtime]/react-refresh/internal/helpers.ts [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[@utoo/pack-runtime]/react-refresh/internal/react-refresh-runtime.development.js [client] (ecmascript)");
;
;
// Hook into ReactDOM initialization
__TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].injectIntoGlobalHook(self);
// Register global helpers
self.$RefreshHelpers$ = __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$helpers$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["default"];
// Register a helper for module execution interception
self.$RefreshInterceptModuleExecution$ = function(webpackModuleId) {
    var prevRefreshReg = self.$RefreshReg$;
    var prevRefreshSig = self.$RefreshSig$;
    self.$RefreshReg$ = function(type, id) {
        __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].register(type, webpackModuleId + " " + id);
    };
    self.$RefreshSig$ = __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$react$2d$refresh$2f$internal$2f$react$2d$refresh$2d$runtime$2e$development$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"].createSignatureFunctionForTransform;
    // Modeled after `useEffect` cleanup pattern:
    // https://react.dev/learn/synchronizing-with-effects#step-3-add-cleanup-if-needed
    return function() {
        self.$RefreshReg$ = prevRefreshReg;
        self.$RefreshSig$ = prevRefreshSig;
    };
};
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[project]/hmr/dynamic_chunk_lists_hot_disabled/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "loadLazy",
    ()=>loadLazy
]);
async function loadLazy() {
    return __turbopack_context__.A("[project]/hmr/dynamic_chunk_lists_hot_disabled/input/lazy.js [client] (ecmascript, async loader)");
}
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
]);