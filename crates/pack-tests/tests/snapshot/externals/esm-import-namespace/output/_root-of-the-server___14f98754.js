(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/native-esm [external] (native-esm, esm_import)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await __turbopack_context__.y("native-esm");

if (mod && mod.__esModule) {
  __turbopack_context__.n(mod);
} else {
  var ns = Object.create(null);
  var isEsmNamespace = mod && typeof Symbol !== 'undefined' && Symbol.toStringTag && mod[Symbol.toStringTag] === 'Module';
  if (mod && (typeof mod === 'object' || typeof mod === 'function')) {
    for (var key in mod) {
      if (key === '__esModule' || (!isEsmNamespace && key === 'default')) continue;
      (function(key) {
        Object.defineProperty(ns, key, { enumerable: true, get: function() { return mod[key]; } });
      })(key);
    }
  }
  if (!isEsmNamespace) {
    Object.defineProperty(ns, 'default', { enumerable: true, value: mod });
  }
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
"[project]/externals/esm-import-namespace/input/index.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
    try {
        var __TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__ = __turbopack_context__.i("[externals]/native-esm [external] (native-esm, esm_import)");
        var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
            __TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__
        ]);
        [__TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
        ;
        (0, __TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__["increment"])();
        console.log(__TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__["named"], __TURBOPACK__imported__module__$5b$externals$5d2f$native$2d$esm__$5b$external$5d$__$28$native$2d$esm$2c$__esm_import$29$__["count"]);
        __turbopack_context__.s([]);
        __turbopack_async_result__();
    } catch (e) {
        __turbopack_async_result__(e);
    }
}, false);
}),
]);

//# sourceMappingURL=_root-of-the-server___14f98754.js.map