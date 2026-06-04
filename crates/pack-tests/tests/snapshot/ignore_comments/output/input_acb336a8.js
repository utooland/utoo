(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/ignore_comments/input/vercel.cjs (static in ecmascript)", ((__turbopack_context__) => {

__turbopack_context__.q("/vercel.fad5a703.cjs");}),
"[project]/ignore_comments/input/vercel.cjs [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = 'turbopack';
}),
"[project]/ignore_comments/input/vercel.cjs [client] (ecmascript, worker loader)", ((__turbopack_context__) => {

__turbopack_context__.v(function(Ctor, opts) {
    return __turbopack_context__.b(Ctor, "turbopack-worker-_client-fs___664a0dec.js", ["input_vercel_cjs_7eef02c4.js","turbopack-input_vercel_cjs_c5ff344e.js"], opts);
});
}),
"[project]/ignore_comments/input/ignore-worker.cjs (static in ecmascript)", ((__turbopack_context__) => {

__turbopack_context__.q("/ignore-worker.4e0cf842.cjs");}),
"[project]/ignore_comments/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__import$2e$meta__ = {
    get url () {
        return __turbopack_context__.F("input/index.js");
    }
};
__turbopack_context__.A("[project]/ignore_comments/input/vercel.mjs [client] (ecmascript, async loader)").then(console.log);
__turbopack_context__.A("[project]/ignore_comments/input/vercel.mjs [client] (ecmascript, async loader)").then(console.log);
console.log(__turbopack_context__.r("[project]/ignore_comments/input/vercel.cjs [client] (ecmascript)"));
__turbopack_context__.r("[project]/ignore_comments/input/vercel.cjs [client] (ecmascript, worker loader)")(Worker);
// turbopack shouldn't attempt to bundle these, and they should be preserved in the output
import(/* webpackIgnore: true */ './ignore.mjs');
import(/* turbopackIgnore: true */ './ignore.mjs');
// this should work for cjs requires too
require(/* webpackIgnore: true */ './ignore.cjs');
require(/* turbopackIgnore: true */ './ignore.cjs');
new Worker(new __turbopack_context__.U(__turbopack_context__.r("[project]/ignore_comments/input/ignore-worker.cjs (static in ecmascript)")));
new Worker(new __turbopack_context__.U(__turbopack_context__.r("[project]/ignore_comments/input/ignore-worker.cjs (static in ecmascript)")));
function foo(plugin) {
    return require(/* turbopackIgnore: true */ plugin);
}
__turbopack_context__.s([
    "foo",
    0,
    foo
]);
}),
"[project]/ignore_comments/input/vercel.mjs [client] (ecmascript, async loader)", ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "input_vercel_mjs_1cb2465a.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport("[project]/ignore_comments/input/vercel.mjs [client] (ecmascript)");
    });
});
}),
]);

//# sourceMappingURL=input_acb336a8.js.map