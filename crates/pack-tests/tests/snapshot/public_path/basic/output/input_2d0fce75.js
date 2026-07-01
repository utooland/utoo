(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
19, ((__turbopack_context__) => {

__turbopack_context__.q("https://cdn.example.com/assets/asset.2d068005.jpg");}),
4, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__19__ = __turbopack_context__.i(19);
;
console.log('Main entry loaded with publicPath');
async function loadLazyModule() {
    const module = await __turbopack_context__.A(78);
    return module.default();
}
function getImageUrl() {
    return __TURBOPACK__imported__module__19__["default"];
}
__turbopack_context__.A(78).then((module)=>{
    console.log(module.default());
});
console.log('Ready to load chunks from CDN');
__turbopack_context__.s([
    "getImageUrl",
    0,
    getImageUrl,
    "loadLazyModule",
    0,
    loadLazyModule
]);
}),
78, ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "input_lazy_be895acc.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport(73);
    });
});
}),
]);

//# sourceMappingURL=input_2d0fce75.js.map