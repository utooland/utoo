(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
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
    0,
    lazyModule,
    "lazyData",
    0,
    lazyData
]);
}),
]);

//# sourceMappingURL=input_lazy_15hx-u408fu-p.js.map