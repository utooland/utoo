(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
50, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "bar",
    ()=>bar,
    "foo",
    ()=>foo
]);
const foo = "foo";
const bar = "bar";
}),
26, ((__turbopack_context__) => {
"use strict";

// import { a } from '@@/a';
// console.log('this is in node_modules', a);
__turbopack_context__.s([
    "a",
    ()=>a,
    "aliasPkg",
    ()=>aliasPkg
]);
const aliasPkg = 'alias-pkg';
;
}),
70, ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__50__ = __turbopack_context__.i(50);
var __TURBOPACK__imported__module__26__ = __turbopack_context__.i(26);
;
;
;
console.log(__TURBOPACK__imported__module__50__["foo"], __TURBOPACK__imported__module__26__["aliasPkg"], __TURBOPACK__imported__module__50__["bar"]);
console.log('xxxx');
}),
]);

//# sourceMappingURL=crates_pack-tests_tests_snapshot_450cdc63.js.map