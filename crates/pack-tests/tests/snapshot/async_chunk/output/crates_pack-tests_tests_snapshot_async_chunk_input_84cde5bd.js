(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
83, ((__turbopack_context__) => {
"use strict";

function bar(value) {
    console.assert(value);
}
__turbopack_context__.s([
    "bar",
    ()=>bar
]);
}),
24, ((__turbopack_context__, module, exports) => {

// shared package
}),
71, ((__turbopack_context__) => {
"use strict";

function foo(value) {
    console.assert(value);
}
__turbopack_context__.s([
    "foo",
    ()=>foo
]);
}),
42, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__71__ = __turbopack_context__.i(71);
var __TURBOPACK__imported__module__83__ = __turbopack_context__.i(83);
var __TURBOPACK__imported__module__24__ = __turbopack_context__.i(24);
;
;
;
(0, __TURBOPACK__imported__module__71__["foo"])(true);
(0, __TURBOPACK__imported__module__83__["bar"])(true);
__turbopack_context__.s([]);
}),
5, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__83__ = __turbopack_context__.i(83);
var __TURBOPACK__imported__module__24__ = __turbopack_context__.i(24);
;
;
(0, __TURBOPACK__imported__module__83__["bar"])(true);
Promise.resolve().then(()=>__turbopack_context__.r(42)).then(({ foo })=>{
    foo(true);
});
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_async_chunk_input_84cde5bd.js.map