(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
27, ((__turbopack_context__) => {
"use strict";

// Imports fnA from pkg and USES it
// Re-exports fnB from pkg
var __TURBOPACK__imported__module__97__ = __turbopack_context__.i(97);
;
;
function useFnA() {
    return (0, __TURBOPACK__imported__module__97__["fnA"])();
}
__turbopack_context__.s([]);
}),
62, ((__turbopack_context__) => {
"use strict";

function fnB() {
    return 'B';
}
__turbopack_context__.s([
    "fnB",
    0,
    fnB
]);
}),
26, ((__turbopack_context__) => {
"use strict";

// Entry only imports fnB, but wrapper.js uses fnA
// BUG: fnA factory is removed because entry doesn't use it
var __TURBOPACK__imported__module__27__ = __turbopack_context__.i(27);
var __TURBOPACK__imported__module__62__ = __turbopack_context__.i(62);
;
console.log((0, __TURBOPACK__imported__module__62__["fnB"])());
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_121f988c.js.map