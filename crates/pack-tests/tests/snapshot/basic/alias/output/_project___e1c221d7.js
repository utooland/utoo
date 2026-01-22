(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
790, ((__turbopack_context__) => {
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
611, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__790__ = __turbopack_context__.i(790);
;
console.log('this is in node_modules', __TURBOPACK__imported__module__790__["a"]);
const aliasPkg = 'alias-pkg';
;
__turbopack_context__.s([
    "aliasPkg",
    0,
    aliasPkg
]);
}),
521, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__790__ = __turbopack_context__.i(790);
__turbopack_context__.s([
    "aliasA",
    ()=>__TURBOPACK__imported__module__790__["a"]
]);
}),
843, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__611__ = __turbopack_context__.i(611);
var __TURBOPACK__imported__module__521__ = __turbopack_context__.i(521);
;
console.log(__TURBOPACK__imported__module__521__["aliasA"]);
;
__turbopack_context__.s([]);
}),
554, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__790__ = __turbopack_context__.i(790);
__turbopack_context__.s([
    "aliasB",
    ()=>__TURBOPACK__imported__module__790__["a"]
]);
}),
706, ((__turbopack_context__) => {
"use strict";

const a = "this is browser";
__turbopack_context__.s([
    "a",
    0,
    a
]);
}),
29, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__706__ = __turbopack_context__.i(706);
;
;
__turbopack_context__.s([]);
}),
734, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__790__ = __turbopack_context__.i(790);
var __TURBOPACK__imported__module__611__ = __turbopack_context__.i(611);
var __TURBOPACK__imported__module__521__ = __turbopack_context__.i(521);
var __TURBOPACK__imported__module__843__ = __turbopack_context__.i(843);
var __TURBOPACK__imported__module__554__ = __turbopack_context__.i(554);
var __TURBOPACK__imported__module__29__ = __turbopack_context__.i(29);
var __TURBOPACK__imported__module__706__ = __turbopack_context__.i(706);
;
;
;
;
;
;
console.log('style', void 0);
console.log(__TURBOPACK__imported__module__706__["a"], __TURBOPACK__imported__module__790__["foo"], __TURBOPACK__imported__module__611__["aliasPkg"], __TURBOPACK__imported__module__790__["bar"]);
console.log('a from alias-pkg', __TURBOPACK__imported__module__521__["aliasA"]);
console.log('b from alias-pkg', __TURBOPACK__imported__module__554__["aliasB"]);
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=_project___e1c221d7.js.map