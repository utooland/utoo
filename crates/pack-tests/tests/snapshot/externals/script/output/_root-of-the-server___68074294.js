(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/EsmScript [external] (EsmScript@https://example.com/esm-script.js, script, async loader)", ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "_externals__EsmScript_67c7ec39.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport("[externals]/EsmScript [external] (EsmScript@https://example.com/esm-script.js, script)");
    });
});
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
"[project]/externals/script/input/index.ts [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

const func = async ()=>{
    // @ts-ignore
    const _ = await __turbopack_context__.A("[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script, async loader)");
    console.log(Object.keys(_.default.omit({
        a: 1
    }, 'a')).length === 0);
    const esm = await __turbopack_context__.A("[externals]/EsmScript [external] (EsmScript@https://example.com/esm-script.js, script, async loader)");
    console.log(esm.default, esm.named);
};
func();
}),
]);

//# sourceMappingURL=_root-of-the-server___68074294.js.map