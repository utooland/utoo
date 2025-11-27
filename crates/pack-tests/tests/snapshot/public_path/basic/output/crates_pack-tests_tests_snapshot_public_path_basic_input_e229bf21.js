(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push(["crates_pack-tests_tests_snapshot_public_path_basic_input_e229bf21.js",
34, ((__turbopack_context__) => {

__turbopack_context__.v("https://cdn.example.com/assets/asset.c77b3abb.jpg");}),
75, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__34__ = __turbopack_context__.i(34);
;
console.log('Main entry loaded with publicPath');
async function loadLazyModule() {
    const module = await __turbopack_context__.A(87);
    return module.default();
}
function getImageUrl() {
    return __TURBOPACK__imported__module__34__["default"];
}
__turbopack_context__.A(87).then((module)=>{
    console.log(module.default());
});
console.log('Ready to load chunks from CDN');
__turbopack_context__.s([
    "getImageUrl",
    ()=>getImageUrl,
    "loadLazyModule",
    ()=>loadLazyModule
]);
}),
87, ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "crates_pack-tests_tests_snapshot_public_path_basic_input_lazy_f10ad965.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport(5);
    });
});
}),
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_public_path_basic_input_e229bf21.js.map