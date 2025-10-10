(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
50, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "a",
    ()=>a,
    "bar",
    ()=>bar,
    "foo",
    ()=>foo
]);
const foo = "foo";
const bar = "bar";
const a = "a from alias test";
}),
5, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "aliasPkg",
    ()=>aliasPkg
]);
var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
;
console.log('this is in node_modules', __TURBOPACK__imported__module__50__["a"]);
const aliasPkg = 'alias-pkg';
;
}),
44, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "aliasA",
    ()=>__TURBOPACK__imported__module__50__["a"]
]);
var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
}),
171, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__5__ = __turbopack_context__.i(5);
var __TURBOPACK__imported__module__44__ = __turbopack_context__.i(44);
;
console.log(__TURBOPACK__imported__module__44__["aliasA"]);
;
}),
933, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "aliasB",
    ()=>__TURBOPACK__imported__module__50__["a"]
]);
var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
}),
70, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
var __TURBOPACK__imported__module__5__ = __turbopack_context__.i(5);
var __TURBOPACK__imported__module__44__ = __turbopack_context__.i(44);
var __TURBOPACK__imported__module__171__ = __turbopack_context__.i(171);
var __TURBOPACK__imported__module__933__ = __turbopack_context__.i(933);
;
;
;
;
console.log(__TURBOPACK__imported__module__50__["foo"], __TURBOPACK__imported__module__5__["aliasPkg"], __TURBOPACK__imported__module__50__["bar"]);
console.log('a from alias-pkg', __TURBOPACK__imported__module__44__["aliasA"]);
console.log('b from alias-pkg', __TURBOPACK__imported__module__933__["aliasB"]);
}),
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_d4ef97b5.js.map