((typeof globalThis !== "undefined" ? globalThis : (typeof self !== "undefined" ? self : (typeof window !== "undefined" ? window : (typeof global !== "undefined" ? global : {}))))["TURBOPACK"] || ((typeof globalThis !== "undefined" ? globalThis : (typeof self !== "undefined" ? self : (typeof window !== "undefined" ? window : (typeof global !== "undefined" ? global : {}))))["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script, async loader)", (function(__turbopack_context__) {

__turbopack_context__.v(function(parentImport) {
    return Promise.all([
  "_externals____2fc6bb03.js"
].map(function(chunk) { return __turbopack_context__.l(chunk); })).then(function() {
        return parentImport("[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script)");
    });
});
}),
"[project]/externals/async-script-externals/input/index.ts [client] (ecmascript)", (function(__turbopack_context__, module, exports) {

const func = async ()=>{
    // @ts-ignore
    const _ = await __turbopack_context__.A("[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script, async loader)");
    console.log(Object.keys(_.default.omit({
        a: 1
    }, 'a')).length === 0);
};
func();
}),
]);

//# sourceMappingURL=_root-of-the-server___d1c9390f.js.map