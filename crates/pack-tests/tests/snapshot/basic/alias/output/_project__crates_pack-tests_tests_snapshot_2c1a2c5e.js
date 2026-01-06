(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
50, ((__turbopack_context__) => {
"use strict";

const foo = "foo";
const bar = "bar";
const a = "a from alias test";
__turbopack_context__.s([
    "a",
    0,
    a,
    "bar",
    0,
    bar,
    "foo",
    0,
    foo
]);
}),
5, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
;
console.log('this is in node_modules', __TURBOPACK__imported__module__50__["a"]);
const aliasPkg = 'alias-pkg';
;
__turbopack_context__.s([
    "aliasPkg",
    0,
    aliasPkg
]);
}),
44, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
__turbopack_context__.s([
    "aliasA",
    ()=>__TURBOPACK__imported__module__50__["a"]
]);
}),
171, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__5__ = __turbopack_context__.i(5);
var __TURBOPACK__imported__module__44__ = __turbopack_context__.i(44);
;
console.log(__TURBOPACK__imported__module__44__["aliasA"]);
;
__turbopack_context__.s([]);
}),
933, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
__turbopack_context__.s([
    "aliasB",
    ()=>__TURBOPACK__imported__module__50__["a"]
]);
}),
64, ((__turbopack_context__) => {
"use strict";

const a = "this is browser";
__turbopack_context__.s([
    "a",
    0,
    a
]);
}),
766, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__64__ = __turbopack_context__.i(64);
;
;
__turbopack_context__.s([]);
}),
70, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
var __TURBOPACK__imported__module__5__ = __turbopack_context__.i(5);
var __TURBOPACK__imported__module__44__ = __turbopack_context__.i(44);
var __TURBOPACK__imported__module__171__ = __turbopack_context__.i(171);
var __TURBOPACK__imported__module__933__ = __turbopack_context__.i(933);
var __TURBOPACK__imported__module__766__ = __turbopack_context__.i(766);
var __TURBOPACK__imported__module__64__ = __turbopack_context__.i(64);
;
;
;
;
;
console.log(__TURBOPACK__imported__module__64__["a"], __TURBOPACK__imported__module__50__["foo"], __TURBOPACK__imported__module__5__["aliasPkg"], __TURBOPACK__imported__module__50__["bar"]);
console.log('a from alias-pkg', __TURBOPACK__imported__module__44__["aliasA"]);
console.log('b from alias-pkg', __TURBOPACK__imported__module__933__["aliasB"]);
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=_project__crates_pack-tests_tests_snapshot_2c1a2c5e.js.map