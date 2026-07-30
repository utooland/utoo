(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/JSZip [external] (JSZip@https://gw.alipayobjects.com/os/lib/jszip/3.10.1/dist/jszip.min.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["JSZip"] !== 'undefined') {
    return globalThis["JSZip"];
  }
  await __turbopack_context__.S("https://gw.alipayobjects.com/os/lib/jszip/3.10.1/dist/jszip.min.js");
  if (typeof globalThis["JSZip"] !== 'undefined') {
    return globalThis["JSZip"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://gw.alipayobjects.com/os/lib/jszip/3.10.1/dist/jszip.min.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://gw.alipayobjects.com/os/lib/jszip/3.10.1/dist/jszip.min.js";
  throw error;
})();

__turbopack_context__.n(__turbopack_context__.N(mod));
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
"[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script, async loader)", ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "_externals____2fc6bb03.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport("[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script)");
    });
});
}),
"[project]/externals/async-script-externals/input/index.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
    try {
        var __TURBOPACK__imported__module__$5b$externals$5d2f$JSZip__$5b$external$5d$__$28$JSZip$40$https$3a2f2f$gw$2e$alipayobjects$2e$com$2f$os$2f$lib$2f$jszip$2f$3$2e$10$2e$1$2f$dist$2f$jszip$2e$min$2e$js$2c$__script$29$__ = __turbopack_context__.i("[externals]/JSZip [external] (JSZip@https://gw.alipayobjects.com/os/lib/jszip/3.10.1/dist/jszip.min.js, script)");
        var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
            __TURBOPACK__imported__module__$5b$externals$5d2f$JSZip__$5b$external$5d$__$28$JSZip$40$https$3a2f2f$gw$2e$alipayobjects$2e$com$2f$os$2f$lib$2f$jszip$2f$3$2e$10$2e$1$2f$dist$2f$jszip$2e$min$2e$js$2c$__script$29$__
        ]);
        [__TURBOPACK__imported__module__$5b$externals$5d2f$JSZip__$5b$external$5d$__$28$JSZip$40$https$3a2f2f$gw$2e$alipayobjects$2e$com$2f$os$2f$lib$2f$jszip$2f$3$2e$10$2e$1$2f$dist$2f$jszip$2e$min$2e$js$2c$__script$29$__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
        ;
        const zip = new __TURBOPACK__imported__module__$5b$externals$5d2f$JSZip__$5b$external$5d$__$28$JSZip$40$https$3a2f2f$gw$2e$alipayobjects$2e$com$2f$os$2f$lib$2f$jszip$2f$3$2e$10$2e$1$2f$dist$2f$jszip$2e$min$2e$js$2c$__script$29$__["default"]();
        zip;
        const func = async ()=>{
            // @ts-ignore
            const _ = await __turbopack_context__.A("[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script, async loader)");
            console.log(Object.keys(_.default.omit({
                a: 1
            }, 'a')).length === 0);
        };
        func();
        __turbopack_context__.s([]);
        __turbopack_async_result__();
    } catch (e) {
        __turbopack_async_result__(e);
    }
}, false);
}),
]);

//# sourceMappingURL=_root-of-the-server___d0ad72cc.js.map