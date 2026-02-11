(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
6, ((__turbopack_context__, module, exports) => {

const func = async ()=>{
    // @ts-ignore
    const _ = await __turbopack_context__.A(96);
    console.log(Object.keys(_.default.omit({
        a: 1
    }, 'a')).length === 0);
};
func();
}),
96, ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "_externals____2fc6bb03.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport(20);
    });
});
}),
]);

//# sourceMappingURL=_root-of-the-server___fa01a871.js.map