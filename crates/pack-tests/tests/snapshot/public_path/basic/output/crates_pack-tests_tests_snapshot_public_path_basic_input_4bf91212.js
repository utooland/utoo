(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
34, ((__turbopack_context__) => {

__turbopack_context__.v("https://cdn.example.com/assets/asset.c77b3abb.jpg");}),
5, ((__turbopack_context__) => {
"use strict";

function lazyModule() {
    console.log('Lazy module loaded from CDN!');
    return {
        loaded: true,
        source: 'CDN'
    };
}
const lazyData = {
    message: 'This chunk should be loaded from publicPath'
};
__turbopack_context__.s([
    "default",
    ()=>lazyModule,
    "lazyData",
    0,
    lazyData
]);
}),
75, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__34__ = __turbopack_context__.i(34);
;
console.log('Main entry loaded with publicPath');
async function loadLazyModule() {
    const module = await Promise.resolve().then(()=>__turbopack_context__.r(5));
    return module.default();
}
function getImageUrl() {
    return __TURBOPACK__imported__module__34__["default"];
}
Promise.resolve().then(()=>__turbopack_context__.r(5)).then((module)=>{
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
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_public_path_basic_input_4bf91212.js.map