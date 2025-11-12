(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
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
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_public_path_basic_input_lazy_f10ad965.js.map