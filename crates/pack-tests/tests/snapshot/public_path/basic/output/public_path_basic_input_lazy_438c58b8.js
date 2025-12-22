(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
73, ((__turbopack_context__) => {
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

//# sourceMappingURL=public_path_basic_input_lazy_438c58b8.js.map