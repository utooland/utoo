(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
983, ((__turbopack_context__) => {
"use strict";

function bar(value) {
    console.assert(value);
}
__turbopack_context__.s([
    "bar",
    0,
    bar
]);
}),
724, ((__turbopack_context__, module, exports) => {

// shared package
}),
605, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__983__ = __turbopack_context__.i(983);
var __TURBOPACK__imported__module__724__ = __turbopack_context__.i(724);
;
;
(0, __TURBOPACK__imported__module__983__["bar"])(true);
__turbopack_context__.A(649).then(({ foo })=>{
    foo(true);
});
__turbopack_context__.s([]);
}),
649, ((__turbopack_context__) => {

__turbopack_context__.v((parentImport) => {
    return Promise.all([
  "input_3b55c753.js"
].map((chunk) => __turbopack_context__.l(chunk))).then(() => {
        return parentImport(942);
    });
});
}),
]);

//# sourceMappingURL=input_32fba1f1.js.map