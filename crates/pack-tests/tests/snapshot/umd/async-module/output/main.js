((__TURBOPACK__) => {
// Dummy runtime
})([["main.js", {

10: ((__turbopack_context__) => {
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
181: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__10__ = __turbopack_context__.i(10);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__10__
]);
[__TURBOPACK__imported__module__10__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__10__["default"] + " world";
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
92: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__278__ = __turbopack_context__.i(278);
var __TURBOPACK__imported__module__181__ = __turbopack_context__.i(181);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__278__,
    __TURBOPACK__imported__module__181__
]);
[__TURBOPACK__imported__module__278__, __TURBOPACK__imported__module__181__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__278__["default"] + ", " + __TURBOPACK__imported__module__181__["default"];
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
278: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__10__ = __turbopack_context__.i(10);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__10__
]);
[__TURBOPACK__imported__module__10__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__10__["default"] + " world";
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
803: ((__turbopack_context__) => {
"use strict";

// This is the async chunk
var __TURBOPACK__default__export__ = 42;
const nested = ()=>Promise.resolve().then(()=>__turbopack_context__.i(278));
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__,
    "nested",
    0,
    nested
]);
}),
826: ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const result = await __turbopack_context__.r(92);
await Promise.resolve().then(()=>__turbopack_context__.i(803));
result.default;
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
},
{"otherChunks":[],"runtimeModuleIds":[826]},
]]);


//# sourceMappingURL=main.js.map