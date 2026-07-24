(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[@utoo/pack-runtime]/hmr/bootstrap.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$client$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[@utoo/pack-runtime]/hmr/client.ts [client] (ecmascript)");
;
(0, __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$client$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["initHMR"])("TURBOPACK_CHUNK_UPDATE_LISTENERS");
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[@utoo/pack-runtime]/hmr/client.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "initHMR",
    ()=>initHMR
]);
// @ts-ignore
var __TURBOPACK__imported__module__$5b$turbopack$5d2f$browser$2f$dev$2f$hmr$2d$client$2f$hmr$2d$client$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[turbopack]/browser/dev/hmr-client/hmr-client.ts [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$websocket$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[@utoo/pack-runtime]/hmr/websocket.ts [client] (ecmascript)");
;
;
function initHMR(chunkUpdateListenersGlobal = "TURBOPACK_CHUNK_UPDATE_LISTENERS") {
    (0, __TURBOPACK__imported__module__$5b$turbopack$5d2f$browser$2f$dev$2f$hmr$2d$client$2f$hmr$2d$client$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["connect"])({
        addMessageListener: __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$websocket$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["addMessageListener"],
        sendMessage: __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$websocket$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["sendMessage"],
        onUpdateError: console.error,
        chunkUpdateListenersGlobal
    });
    (0, __TURBOPACK__imported__module__$5b40$utoo$2f$pack$2d$runtime$5d2f$hmr$2f$websocket$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["connectHMR"])({
        path: "/turbopack-hmr"
    });
}
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[@utoo/pack-runtime]/hmr/websocket.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

// Adapted from https://github.com/vercel/next.js/blob/canary/packages/next/src/client/dev/error-overlay/websocket.ts
__turbopack_context__.s([
    "addMessageListener",
    ()=>addMessageListener,
    "connectHMR",
    ()=>connectHMR,
    "sendMessage",
    ()=>sendMessage
]);
let source = null;
let eventCallbacks = [];
const INITIAL_RECONNECT_DELAY_MS = 500;
const MAX_RECONNECT_DELAY_MS = 5_000;
// Helper function to dispatch messages to all event callbacks
function dispatchMessage(message) {
    for (const eventCallback of eventCallbacks){
        eventCallback(message);
    }
}
function addMessageListener(callback) {
    eventCallbacks.push(callback);
}
function sendMessage(data) {
    if (source && source.readyState === source.OPEN) {
        const message = typeof data === "string" ? data : JSON.stringify(data);
        source.send(message);
    }
}
function getSocketProtocol() {
    return typeof location !== "undefined" && location.protocol === "https:" ? "wss" : "ws";
}
function getSocketUrl() {
    const socketServer = ("TURBOPACK compile-time value", undefined);
    if (socketServer) {
        try {
            const parsed = new URL(socketServer);
            const protocol = parsed.protocol === "https:" ? "wss:" : parsed.protocol === "http:" ? "ws:" : parsed.protocol;
            if (protocol === "ws:" || protocol === "wss:") {
                const pathname = parsed.pathname === "/" ? "" : parsed.pathname.replace(/\/+$/, "");
                return `${protocol}//${parsed.host}${pathname}`;
            }
        } catch  {}
    }
    const { hostname, port } = location;
    const protocol = getSocketProtocol();
    return `${protocol}://${hostname}${port ? `:${port}` : ""}`;
}
let reloading = false;
let serverSessionId = null;
function connectHMR(options) {
    let reconnectTimer = null;
    let reconnectAttempts = 0;
    let pageIsUnloading = false;
    function clearReconnectTimer() {
        if (reconnectTimer !== null) {
            clearTimeout(reconnectTimer);
            reconnectTimer = null;
        }
    }
    function closeSocket(socket) {
        socket.onopen = null;
        socket.onerror = null;
        socket.onclose = null;
        socket.onmessage = null;
        if (source === socket) {
            source = null;
        }
        if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
            socket.close();
        }
    }
    function scheduleReconnect() {
        if (reconnectTimer !== null || pageIsUnloading || reloading) {
            return;
        }
        const delay = Math.min(INITIAL_RECONNECT_DELAY_MS * 2 ** reconnectAttempts, MAX_RECONNECT_DELAY_MS);
        reconnectAttempts += 1;
        reconnectTimer = setTimeout(()=>{
            reconnectTimer = null;
            init();
        }, delay);
    }
    function init() {
        if (pageIsUnloading || reloading) {
            return;
        }
        if (source) {
            closeSocket(source);
        }
        console.log("[HMR] connecting...");
        let socket;
        try {
            socket = new WebSocket(`${getSocketUrl()}${options.path}`);
        } catch (error) {
            console.error("[HMR] Failed to create WebSocket:", error);
            scheduleReconnect();
            return;
        }
        source = socket;
        function handleOnline() {
            if (source !== socket || pageIsUnloading || reloading) {
                closeSocket(socket);
                return;
            }
            reconnectAttempts = 0;
            window.console.log("[HMR] connected");
            // Direct turbopack-dev-server does not send a separate connected frame.
            // Utoo does, but the socket-open notification is sufficient to restore
            // subscriptions in both cases.
            dispatchMessage({
                type: "turbopack-connected"
            });
        }
        function handleMessage(event) {
            if (source !== socket || reloading) {
                return;
            }
            try {
                const msg = JSON.parse(event.data);
                // Handle the different message formats from different servers
                if (msg.action === "turbopack-connected") {
                    if (serverSessionId !== null && serverSessionId !== msg.data.sessionId) {
                        reloading = true;
                        window.location.reload();
                        return;
                    }
                    serverSessionId = msg.data.sessionId;
                    // Socket open already restored subscriptions. This frame only carries
                    // the Utoo server session id used to detect server restarts.
                    return;
                }
                if (msg.action === "reload") {
                    reloading = true;
                    window.location.reload();
                    return;
                }
                if (msg.action === "turbopack-message") {
                    const turbopackMessage = {
                        type: "turbopack-message",
                        data: msg.data
                    };
                    dispatchMessage(turbopackMessage);
                    return;
                }
                // Handle direct turbopack-dev-server messages
                if (msg.type && [
                    "partial",
                    "restart",
                    "notFound",
                    "issues"
                ].includes(msg.type)) {
                    const turbopackMessage = {
                        type: "turbopack-message",
                        data: msg
                    };
                    dispatchMessage(turbopackMessage);
                    return;
                }
            // TODO: handle rest msg.actions
            } catch (e) {
                console.error("[HMR] Failed to parse message:", e);
            }
        }
        function handleDisconnect(event) {
            if (event.target !== socket || source !== socket) {
                return;
            }
            closeSocket(socket);
            window.console.warn("[HMR] disconnected");
            scheduleReconnect();
        }
        socket.onopen = handleOnline;
        socket.onerror = handleDisconnect;
        socket.onclose = handleDisconnect;
        socket.onmessage = handleMessage;
    }
    // `pagehide` is not fired when a beforeunload prompt is cancelled, so it is
    // safe to use as the point where reconnects should stop.
    window.addEventListener("pagehide", ()=>{
        pageIsUnloading = true;
        clearReconnectTimer();
        if (source) {
            closeSocket(source);
        }
    });
    // A page restored from the back-forward cache needs a fresh HMR transport.
    window.addEventListener("pageshow", (event)=>{
        if (event.persisted) {
            pageIsUnloading = false;
            reconnectAttempts = 0;
            init();
        }
    });
    init();
}
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
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
"[project]/hmr/dynamic_chunk_lists/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "loadLazy",
    ()=>loadLazy
]);
async function loadLazy() {
    return __turbopack_context__.A("[project]/hmr/dynamic_chunk_lists/input/lazy.js [client] (ecmascript, async loader)");
}
if (typeof globalThis.$RefreshHelpers$ === 'object' && globalThis.$RefreshHelpers !== null) {
    __turbopack_context__.k.registerExports(__turbopack_context__.m, globalThis.$RefreshHelpers$);
}
}),
"[turbopack]/browser/dev/hmr-client/hmr-client.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

/// <reference path="../../../shared/runtime/runtime-types.d.ts" />
/// <reference path="../../../shared/runtime/dev-globals.d.ts" />
/// <reference path="../../../shared/runtime/dev-protocol.d.ts" />
/// <reference path="../../../shared/runtime/dev-extensions.ts" />
__turbopack_context__.s([
    "TURBOPACK_CHUNK_UPDATE_LISTENERS_GLOBAL",
    ()=>TURBOPACK_CHUNK_UPDATE_LISTENERS_GLOBAL,
    "connect",
    ()=>connect,
    "setHooks",
    ()=>setHooks,
    "subscribeToUpdate",
    ()=>subscribeToUpdate
]);
const TURBOPACK_CHUNK_UPDATE_LISTENERS_GLOBAL = 'TURBOPACK_CHUNK_UPDATE_LISTENERS';
function connect({ addMessageListener, sendMessage, onUpdateError = console.error, chunkUpdateListenersGlobal }) {
    addMessageListener((msg)=>{
        switch(msg.type){
            case 'turbopack-connected':
                handleSocketConnected(sendMessage);
                break;
            default:
                try {
                    if (Array.isArray(msg.data)) {
                        for(let i = 0; i < msg.data.length; i++){
                            handleSocketMessage(msg.data[i]);
                        }
                    } else {
                        handleSocketMessage(msg.data);
                    }
                    applyAggregatedUpdates();
                } catch (e) {
                    console.warn('[Fast Refresh] performing full reload\n\n' + "Fast Refresh will perform a full reload when you edit a file that's imported by modules outside of the React rendering tree.\n" + 'You might have a file which exports a React component but also exports a value that is imported by a non-React component file.\n' + 'Consider migrating the non-React component export to a separate file and importing it into both files.\n\n' + 'It is also possible the parent component of the component you edited is a class component, which disables Fast Refresh.\n' + 'Fast Refresh requires at least one parent function component in your React tree.');
                    onUpdateError(e);
                    location.reload();
                }
                break;
        }
    });
    const global = globalThis;
    const queued = global[chunkUpdateListenersGlobal];
    if (queued != null && !Array.isArray(queued)) {
        throw new Error('A separate HMR handler was already registered');
    }
    global[chunkUpdateListenersGlobal] = {
        push: ([chunkPath, callback, options])=>{
            subscribeToChunkUpdate(chunkPath, sendMessage, callback, options?.conservative, options?.onSubscribed, options?.expectedVersion);
        }
    };
    if (Array.isArray(queued)) {
        for (const [chunkPath, callback, options] of queued){
            subscribeToChunkUpdate(chunkPath, sendMessage, callback, options?.conservative, options?.onSubscribed, options?.expectedVersion);
        }
    }
}
const updateCallbackSets = new Map();
function sendJSON(sendMessage, message) {
    sendMessage(JSON.stringify(message));
}
function resourceKey(resource) {
    return JSON.stringify({
        path: resource.path,
        headers: resource.headers || null
    });
}
function createValidationToken() {
    return `${Date.now()}:${Math.random()}`;
}
function subscribeToUpdates(sendMessage, resource, expectedVersion, validation) {
    sendJSON(sendMessage, {
        type: 'turbopack-subscribe',
        ...resource,
        version: expectedVersion,
        validation
    });
    return ()=>{
        sendJSON(sendMessage, {
            type: 'turbopack-unsubscribe',
            ...resource
        });
    };
}
function handleSocketConnected(sendMessage) {
    for (const [key, callbackSet] of updateCallbackSets){
        callbackSet.subscribed = false;
        if (callbackSet.validationToken !== undefined) {
            callbackSet.validationToken = createValidationToken();
        }
        callbackSet.unsubscribe = subscribeToUpdates(sendMessage, JSON.parse(key), callbackSet.expectedVersion, callbackSet.validationToken);
    }
}
// we aggregate all pending updates until the issues are resolved
const chunkListsWithPendingUpdates = new Map();
function aggregateUpdates(msg) {
    const key = resourceKey(msg.resource);
    let aggregated = chunkListsWithPendingUpdates.get(key);
    if (aggregated) {
        aggregated.instruction = mergeChunkListUpdates(aggregated.instruction, msg.instruction);
    } else {
        chunkListsWithPendingUpdates.set(key, msg);
    }
}
function applyAggregatedUpdates() {
    if (chunkListsWithPendingUpdates.size === 0) return;
    const updates = [
        ...chunkListsWithPendingUpdates.values()
    ];
    chunkListsWithPendingUpdates.clear();
    if (updates.length > 1 && updates.some((update)=>updateCallbackSets.get(resourceKey(update.resource))?.conservative)) {
        throw new Error('Multiple dynamic HMR lists changed in one build; reloading conservatively.');
    }
    hooks.beforeRefresh();
    for (const msg of updates){
        triggerUpdate(msg);
    }
    finalizeUpdate();
}
function mergeChunkListUpdates(updateA, updateB) {
    let chunks;
    if (updateA.chunks != null) {
        if (updateB.chunks == null) {
            chunks = updateA.chunks;
        } else {
            chunks = mergeChunkListChunks(updateA.chunks, updateB.chunks);
        }
    } else if (updateB.chunks != null) {
        chunks = updateB.chunks;
    }
    let merged;
    if (updateA.merged != null) {
        if (updateB.merged == null) {
            merged = updateA.merged;
        } else {
            // Since `merged` is an array of updates, we need to merge them all into
            // one, consistent update.
            // Since there can only be `EcmascriptMergeUpdates` in the array, there is
            // no need to key on the `type` field.
            let update = updateA.merged[0];
            for(let i = 1; i < updateA.merged.length; i++){
                update = mergeChunkListEcmascriptMergedUpdates(update, updateA.merged[i]);
            }
            for(let i = 0; i < updateB.merged.length; i++){
                update = mergeChunkListEcmascriptMergedUpdates(update, updateB.merged[i]);
            }
            merged = [
                update
            ];
        }
    } else if (updateB.merged != null) {
        merged = updateB.merged;
    }
    return {
        type: 'ChunkListUpdate',
        chunks,
        merged
    };
}
function mergeChunkListChunks(chunksA, chunksB) {
    const chunks = {};
    for (const [chunkPath, chunkUpdateA] of Object.entries(chunksA)){
        const chunkUpdateB = chunksB[chunkPath];
        if (chunkUpdateB != null) {
            const mergedUpdate = mergeChunkUpdates(chunkUpdateA, chunkUpdateB);
            if (mergedUpdate != null) {
                chunks[chunkPath] = mergedUpdate;
            }
        } else {
            chunks[chunkPath] = chunkUpdateA;
        }
    }
    for (const [chunkPath, chunkUpdateB] of Object.entries(chunksB)){
        if (chunks[chunkPath] == null) {
            chunks[chunkPath] = chunkUpdateB;
        }
    }
    return chunks;
}
function mergeChunkUpdates(updateA, updateB) {
    if (updateA.type === 'added' && updateB.type === 'deleted' || updateA.type === 'deleted' && updateB.type === 'added') {
        return undefined;
    }
    if (updateB.type === 'total') {
        // A total update replaces the entire chunk, so it supersedes any prior update.
        return updateB;
    }
    if (updateA.type === 'partial') {
        invariant(updateA.instruction, 'Partial updates are unsupported');
    }
    if (updateB.type === 'partial') {
        invariant(updateB.instruction, 'Partial updates are unsupported');
    }
    return undefined;
}
function mergeChunkListEcmascriptMergedUpdates(mergedA, mergedB) {
    const entries = mergeEcmascriptChunkEntries(mergedA.entries, mergedB.entries);
    const chunks = mergeEcmascriptChunksUpdates(mergedA.chunks, mergedB.chunks);
    return {
        type: 'EcmascriptMergedUpdate',
        entries,
        chunks
    };
}
function mergeEcmascriptChunkEntries(entriesA, entriesB) {
    return {
        ...entriesA,
        ...entriesB
    };
}
function mergeEcmascriptChunksUpdates(chunksA, chunksB) {
    if (chunksA == null) {
        return chunksB;
    }
    if (chunksB == null) {
        return chunksA;
    }
    const chunks = {};
    for (const [chunkPath, chunkUpdateA] of Object.entries(chunksA)){
        const chunkUpdateB = chunksB[chunkPath];
        if (chunkUpdateB != null) {
            const mergedUpdate = mergeEcmascriptChunkUpdates(chunkUpdateA, chunkUpdateB);
            if (mergedUpdate != null) {
                chunks[chunkPath] = mergedUpdate;
            }
        } else {
            chunks[chunkPath] = chunkUpdateA;
        }
    }
    for (const [chunkPath, chunkUpdateB] of Object.entries(chunksB)){
        if (chunks[chunkPath] == null) {
            chunks[chunkPath] = chunkUpdateB;
        }
    }
    if (Object.keys(chunks).length === 0) {
        return undefined;
    }
    return chunks;
}
function mergeEcmascriptChunkUpdates(updateA, updateB) {
    if (updateA.type === 'added' && updateB.type === 'deleted') {
        // These two completely cancel each other out.
        return undefined;
    }
    if (updateA.type === 'deleted' && updateB.type === 'added') {
        const added = [];
        const deleted = [];
        const deletedModules = new Set(updateA.modules ?? []);
        const addedModules = new Set(updateB.modules ?? []);
        for (const moduleId of addedModules){
            if (!deletedModules.has(moduleId)) {
                added.push(moduleId);
            }
        }
        for (const moduleId of deletedModules){
            if (!addedModules.has(moduleId)) {
                deleted.push(moduleId);
            }
        }
        if (added.length === 0 && deleted.length === 0) {
            return undefined;
        }
        return {
            type: 'partial',
            added,
            deleted
        };
    }
    if (updateA.type === 'partial' && updateB.type === 'partial') {
        const added = new Set([
            ...updateA.added ?? [],
            ...updateB.added ?? []
        ]);
        const deleted = new Set([
            ...updateA.deleted ?? [],
            ...updateB.deleted ?? []
        ]);
        if (updateB.added != null) {
            for (const moduleId of updateB.added){
                deleted.delete(moduleId);
            }
        }
        if (updateB.deleted != null) {
            for (const moduleId of updateB.deleted){
                added.delete(moduleId);
            }
        }
        return {
            type: 'partial',
            added: [
                ...added
            ],
            deleted: [
                ...deleted
            ]
        };
    }
    if (updateA.type === 'added' && updateB.type === 'partial') {
        const modules = new Set([
            ...updateA.modules ?? [],
            ...updateB.added ?? []
        ]);
        for (const moduleId of updateB.deleted ?? []){
            modules.delete(moduleId);
        }
        return {
            type: 'added',
            modules: [
                ...modules
            ]
        };
    }
    if (updateA.type === 'partial' && updateB.type === 'deleted') {
        // We could eagerly return `updateB` here, but this would potentially be
        // incorrect if `updateA` has added modules.
        const modules = new Set(updateB.modules ?? []);
        if (updateA.added != null) {
            for (const moduleId of updateA.added){
                modules.delete(moduleId);
            }
        }
        return {
            type: 'deleted',
            modules: [
                ...modules
            ]
        };
    }
    // Any other update combination is invalid.
    return undefined;
}
function invariant(_, message) {
    throw new Error(`Invariant: ${message}`);
}
const CRITICAL = [
    'bug',
    'error',
    'fatal'
];
function compareByList(list, a, b) {
    const aI = list.indexOf(a) + 1 || list.length;
    const bI = list.indexOf(b) + 1 || list.length;
    return aI - bI;
}
const chunksWithIssues = new Map();
function emitIssues() {
    const issues = [];
    const deduplicationSet = new Set();
    for (const [_, chunkIssues] of chunksWithIssues){
        for (const chunkIssue of chunkIssues){
            if (deduplicationSet.has(chunkIssue.formatted)) continue;
            issues.push(chunkIssue);
            deduplicationSet.add(chunkIssue.formatted);
        }
    }
    sortIssues(issues);
    hooks.issues(issues);
}
function handleIssues(msg) {
    const key = resourceKey(msg.resource);
    let hasCriticalIssues = false;
    for (const issue of msg.issues){
        if (CRITICAL.includes(issue.severity)) {
            hasCriticalIssues = true;
        }
    }
    if (msg.issues.length > 0) {
        chunksWithIssues.set(key, msg.issues);
    } else if (chunksWithIssues.has(key)) {
        chunksWithIssues.delete(key);
    }
    emitIssues();
    return hasCriticalIssues;
}
const SEVERITY_ORDER = [
    'bug',
    'fatal',
    'error',
    'warning',
    'info',
    'log'
];
const CATEGORY_ORDER = [
    'parse',
    'resolve',
    'code generation',
    'rendering',
    'typescript',
    'other'
];
function sortIssues(issues) {
    issues.sort((a, b)=>{
        const first = compareByList(SEVERITY_ORDER, a.severity, b.severity);
        if (first !== 0) return first;
        return compareByList(CATEGORY_ORDER, a.category, b.category);
    });
}
const hooks = {
    beforeRefresh: ()=>{},
    refresh: ()=>{},
    buildOk: ()=>{},
    issues: (_issues)=>{}
};
function setHooks(newHooks) {
    Object.assign(hooks, newHooks);
}
function handleSocketMessage(msg) {
    const callbackSet = updateCallbackSets.get(resourceKey(msg.resource));
    if (!callbackSet || callbackSet.validationToken !== msg.validation) {
        // Frames already queued by an old subscription can arrive after an
        // unsubscribe. Only the active validation generation may update state.
        return;
    }
    sortIssues(msg.issues);
    const hasCriticalIssues = handleIssues(msg);
    if (!hasCriticalIssues && msg.type === 'issues') {
        markUpdateSubscriptionReady(msg.resource);
    } else if (msg.type !== 'issues') {
        rejectUpdateSubscriptionValidation(msg.resource, new Error(`Received ${msg.type} before the HMR subscription baseline was validated.`));
    }
    switch(msg.type){
        case 'issues':
            break;
        case 'partial':
            // aggregate updates
            aggregateUpdates(msg);
            break;
        default:
            // run single update
            const runHooks = chunkListsWithPendingUpdates.size === 0;
            if (runHooks) hooks.beforeRefresh();
            triggerUpdate(msg);
            if (runHooks) finalizeUpdate();
            break;
    }
}
function markUpdateSubscriptionReady(resource) {
    const callbackSet = updateCallbackSets.get(resourceKey(resource));
    if (!callbackSet || callbackSet.subscribed) return;
    callbackSet.subscribed = true;
    callbackSet.validation?.resolve();
    callbackSet.validation = undefined;
    for (const callback of callbackSet.onSubscribedCallbacks){
        callback(callbackSet.revalidate);
    }
    callbackSet.onSubscribedCallbacks.clear();
}
function rejectUpdateSubscriptionValidation(resource, error) {
    const callbackSet = updateCallbackSets.get(resourceKey(resource));
    if (!callbackSet || callbackSet.subscribed) return;
    callbackSet.validation?.reject(error);
    callbackSet.validation = undefined;
}
function finalizeUpdate() {
    hooks.refresh();
    hooks.buildOk();
    // This is used by the Next.js integration test suite to notify it when HMR
    // updates have been completed.
    // TODO: Only run this in test environments (gate by `process.env.__NEXT_TEST_MODE`)
    if (globalThis.__NEXT_HMR_CB) {
        globalThis.__NEXT_HMR_CB();
        globalThis.__NEXT_HMR_CB = null;
    }
}
function subscribeToChunkUpdate(chunkListPath, sendMessage, callback, conservative, onSubscribed, expectedVersion) {
    return subscribeToUpdate({
        path: chunkListPath
    }, sendMessage, callback, conservative, onSubscribed, expectedVersion);
}
function subscribeToUpdate(resource, sendMessage, callback, conservative, onSubscribed, expectedVersion) {
    const key = resourceKey(resource);
    let callbackSet;
    const existingCallbackSet = updateCallbackSets.get(key);
    if (!existingCallbackSet) {
        let callbackSetRef;
        const revalidate = ()=>{
            if (callbackSetRef.validation) return callbackSetRef.validation.promise;
            if (updateCallbackSets.get(key) !== callbackSetRef) {
                return Promise.reject(new Error(`Cannot validate an inactive HMR subscription for ${resource.path}.`));
            }
            callbackSetRef.unsubscribe();
            callbackSetRef.subscribed = false;
            callbackSetRef.validationToken = createValidationToken();
            let resolve;
            let reject;
            const promise = new Promise((innerResolve, innerReject)=>{
                resolve = innerResolve;
                reject = innerReject;
            });
            callbackSetRef.validation = {
                promise,
                resolve,
                reject
            };
            callbackSetRef.unsubscribe = subscribeToUpdates(sendMessage, resource, expectedVersion, callbackSetRef.validationToken);
            return promise;
        };
        const validationToken = onSubscribed ? createValidationToken() : undefined;
        callbackSet = {
            callbacks: new Set([
                callback
            ]),
            conservative: Boolean(conservative),
            onSubscribedCallbacks: new Set(onSubscribed ? [
                onSubscribed
            ] : []),
            subscribed: false,
            expectedVersion,
            validationToken,
            revalidate,
            unsubscribe: subscribeToUpdates(sendMessage, resource, expectedVersion, validationToken)
        };
        callbackSetRef = callbackSet;
        updateCallbackSets.set(key, callbackSet);
    } else {
        if (existingCallbackSet.expectedVersion !== expectedVersion) {
            location.reload();
            return ()=>{};
        }
        existingCallbackSet.callbacks.add(callback);
        existingCallbackSet.conservative ||= Boolean(conservative);
        if (onSubscribed) {
            if (existingCallbackSet.subscribed) {
                onSubscribed(existingCallbackSet.revalidate);
            } else {
                existingCallbackSet.onSubscribedCallbacks.add(onSubscribed);
            }
        }
        callbackSet = existingCallbackSet;
    }
    return ()=>{
        callbackSet.callbacks.delete(callback);
        if (onSubscribed) callbackSet.onSubscribedCallbacks.delete(onSubscribed);
        if (callbackSet.callbacks.size === 0) {
            callbackSet.unsubscribe();
            updateCallbackSets.delete(key);
            callbackSet.validationToken = undefined;
            callbackSet.validation?.reject(new Error(`HMR subscription for ${resource.path} was removed.`));
            callbackSet.validation = undefined;
        }
    };
}
function triggerUpdate(msg) {
    const key = resourceKey(msg.resource);
    const callbackSet = updateCallbackSets.get(key);
    if (!callbackSet) {
        return;
    }
    for (const callback of callbackSet.callbacks){
        callback(msg);
    }
    if (msg.type === 'notFound') {
        // This indicates that the resource which we subscribed to either does not exist or
        // has been deleted. In either case, we should clear all update callbacks, so if a
        // new subscription is created for the same resource, it will send a new "subscribe"
        // message to the server.
        // No need to send an "unsubscribe" message to the server, it will have already
        // dropped the update stream before sending the "notFound" message.
        updateCallbackSets.delete(key);
    }
}
}),
]);