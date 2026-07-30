(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/bar [external] (bar, global)", ((__turbopack_context__) => {
"use strict";

var mod = globalThis["bar"];

__turbopack_context__.v(mod);
}),
"[externals]/bar [external] (bar, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("bar", () => require("bar"));

module.exports = mod;
}),
"[externals]/bar_require2 [external] (bar_require2, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("bar_require2", () => require("bar_require2"));

module.exports = mod;
}),
"[externals]/bar [external] (bar, esm_import)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await __turbopack_context__.y("bar");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
"[externals]/bar_import2 [external] (bar_import2, esm_import)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await __turbopack_context__.y("bar_import2");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
"[externals]/bar_script1 [external] (bar_script1@https://example.com/lib/script.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["bar_script1"] !== 'undefined') {
    return globalThis["bar_script1"];
  }
  await __turbopack_context__.S("https://example.com/lib/script.js");
  if (typeof globalThis["bar_script1"] !== 'undefined') {
    return globalThis["bar_script1"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/lib/script.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/lib/script.js";
  throw error;
})();

if (mod && mod.__esModule) {
  __turbopack_context__.n(mod);
} else {
  var ns = Object.create(null);
  if (mod && (typeof mod === 'object' || typeof mod === 'function')) {
    for (var key in mod) ns[key] = mod[key];
  }
  ns.default = mod;
  Object.defineProperty(ns, '__esModule', { value: true });
  if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(ns, Symbol.toStringTag, { value: 'Module' });
  }
  __turbopack_context__.n(ns);
}
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
"[externals]/bar_script2 [external] (bar_script2@https://example.com/lib/script2.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["bar_script2"] !== 'undefined') {
    return globalThis["bar_script2"];
  }
  await __turbopack_context__.S("https://example.com/lib/script2.js");
  if (typeof globalThis["bar_script2"] !== 'undefined') {
    return globalThis["bar_script2"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/lib/script2.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/lib/script2.js";
  throw error;
})();

if (mod && mod.__esModule) {
  __turbopack_context__.n(mod);
} else {
  var ns = Object.create(null);
  if (mod && (typeof mod === 'object' || typeof mod === 'function')) {
    for (var key in mod) ns[key] = mod[key];
  }
  ns.default = mod;
  Object.defineProperty(ns, '__esModule', { value: true });
  if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(ns, Symbol.toStringTag, { value: 'Module' });
  }
  __turbopack_context__.n(ns);
}
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
"[project]/externals/basic/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
    try {
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__global$29$__ = __turbopack_context__.i("[externals]/bar [external] (bar, global)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/bar [external] (bar, cjs)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar_require2__$5b$external$5d$__$28$bar_require2$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/bar_require2 [external] (bar_require2, cjs)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__esm_import$29$__ = __turbopack_context__.i("[externals]/bar [external] (bar, esm_import)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar_import2__$5b$external$5d$__$28$bar_import2$2c$__esm_import$29$__ = __turbopack_context__.i("[externals]/bar_import2 [external] (bar_import2, esm_import)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script1__$5b$external$5d$__$28$bar_script1$40$https$3a2f2f$example$2e$com$2f$lib$2f$script$2e$js$2c$__script$29$__ = __turbopack_context__.i("[externals]/bar_script1 [external] (bar_script1@https://example.com/lib/script.js, script)");
        var __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script2__$5b$external$5d$__$28$bar_script2$40$https$3a2f2f$example$2e$com$2f$lib$2f$script2$2e$js$2c$__script$29$__ = __turbopack_context__.i("[externals]/bar_script2 [external] (bar_script2@https://example.com/lib/script2.js, script)");
        var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
            __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__esm_import$29$__,
            __TURBOPACK__imported__module__$5b$externals$5d2f$bar_import2__$5b$external$5d$__$28$bar_import2$2c$__esm_import$29$__,
            __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script1__$5b$external$5d$__$28$bar_script1$40$https$3a2f2f$example$2e$com$2f$lib$2f$script$2e$js$2c$__script$29$__,
            __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script2__$5b$external$5d$__$28$bar_script2$40$https$3a2f2f$example$2e$com$2f$lib$2f$script2$2e$js$2c$__script$29$__
        ]);
        [__TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__esm_import$29$__, __TURBOPACK__imported__module__$5b$externals$5d2f$bar_import2__$5b$external$5d$__$28$bar_import2$2c$__esm_import$29$__, __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script1__$5b$external$5d$__$28$bar_script1$40$https$3a2f2f$example$2e$com$2f$lib$2f$script$2e$js$2c$__script$29$__, __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script2__$5b$external$5d$__$28$bar_script2$40$https$3a2f2f$example$2e$com$2f$lib$2f$script2$2e$js$2c$__script$29$__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
        ;
        ;
        ;
        ;
        ;
        ;
        ;
        console.log(__TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__global$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__cjs$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$bar_require2__$5b$external$5d$__$28$bar_require2$2c$__cjs$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$bar__$5b$external$5d$__$28$bar$2c$__esm_import$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$bar_import2__$5b$external$5d$__$28$bar_import2$2c$__esm_import$29$__["default"]);
        console.log(__TURBOPACK__imported__module__$5b$externals$5d2f$bar_script1__$5b$external$5d$__$28$bar_script1$40$https$3a2f2f$example$2e$com$2f$lib$2f$script$2e$js$2c$__script$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$bar_script2__$5b$external$5d$__$28$bar_script2$40$https$3a2f2f$example$2e$com$2f$lib$2f$script2$2e$js$2c$__script$29$__["default"]);
        __turbopack_context__.s([]);
        __turbopack_async_result__();
    } catch (e) {
        __turbopack_async_result__(e);
    }
}, false);
}),
]);

//# sourceMappingURL=_root-of-the-server___3b647096.js.map