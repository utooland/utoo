((__TURBOPACK__) => {
// Dummy runtime
})([["main.js", {

99: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

await 1;
await 1;
var __TURBOPACK__default__export__ = "hello";
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
890: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__99__ = __turbopack_context__.i(99);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__99__
]);
[__TURBOPACK__imported__module__99__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__99__["default"] + " world";
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
636: ((__turbopack_context__) => {
"use strict";

// This is the async chunk
var __TURBOPACK__default__export__ = 42;
const nested = ()=>Promise.resolve().then(()=>__turbopack_context__.i(890));
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__,
    "nested",
    0,
    nested
]);
}),
152: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__99__ = __turbopack_context__.i(99);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__99__
]);
[__TURBOPACK__imported__module__99__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__99__["default"] + " world";
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
386: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__890__ = __turbopack_context__.i(890);
var __TURBOPACK__imported__module__152__ = __turbopack_context__.i(152);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__890__,
    __TURBOPACK__imported__module__152__
]);
[__TURBOPACK__imported__module__890__, __TURBOPACK__imported__module__152__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__890__["default"] + ", " + __TURBOPACK__imported__module__152__["default"];
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
111: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const result = await __turbopack_context__.r(386);
await Promise.resolve().then(()=>__turbopack_context__.i(636));
result.default;
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
},
{"otherChunks":[],"runtimeModuleIds":[111]},
]]);


//# sourceMappingURL=main.js.map