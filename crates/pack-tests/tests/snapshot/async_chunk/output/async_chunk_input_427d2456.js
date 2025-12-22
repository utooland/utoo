(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
402, ((__turbopack_context__) => {
"use strict";

function bar(value) {
    console.assert(value);
}
__turbopack_context__.s([
    "bar",
    ()=>bar
]);
}),
743, ((__turbopack_context__, module, exports) => {

// shared package
}),
545, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__402__ = __turbopack_context__.i(402);
var __TURBOPACK__imported__module__743__ = __turbopack_context__.i(743);
;
;
(0, __TURBOPACK__imported__module__402__["bar"])(true);
__turbopack_context__.A(192).then(({ foo })=>{
    foo(true);
});
__turbopack_context__.s([]);
}),
192, ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "async_chunk_input_79e6c840.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport(476);
    });
});
}),
]);

//# sourceMappingURL=async_chunk_input_427d2456.js.map