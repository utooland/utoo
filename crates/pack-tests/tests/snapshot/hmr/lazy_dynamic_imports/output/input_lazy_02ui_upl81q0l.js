(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/hmr/lazy_dynamic_imports/input/lazy.js [client] (ecmascript, lazy compilation proxy FollowReexports(None), loader)", ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all(["input_lazy_js_lazy-compilation-5fb9052f1abd6c8b_0ol7towz0q1u1.js"].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return __turbopack_context__.r("[project]/hmr/lazy_dynamic_imports/input/lazy.js.lazy-compilation-5fb9052f1abd6c8b.js [client] (ecmascript, lazy compilation proxy FollowReexports(None), manifest chunk)");
    }).then((chunks) => {
        return Promise.all(chunks.map((chunk) => __turbopack_context__.l(chunk)));
    }).then(() => {
        return parentImport("[project]/hmr/lazy_dynamic_imports/input/lazy.js [client] (ecmascript, lazy compilation proxy FollowReexports(None))");
    });
});
}),
]);