(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/JSZip [external] (JSZip@https://example.com/jszip.js, script, async loader)", ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "_externals__JSZip_1e2s1wqm_7c7i.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport("[externals]/JSZip [external] (JSZip@https://example.com/jszip.js, script)");
    });
});
}),
"[project]/externals/script-cjs-namespace/input/index.ts [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

const load = async ()=>{
    const jszip = await __turbopack_context__.A("[externals]/JSZip [external] (JSZip@https://example.com/jszip.js, script, async loader)");
    console.log(jszip.default, jszip.version);
};
load();
}),
]);

//# sourceMappingURL=_root-of-the-server___183lg4qim4biy.js.map