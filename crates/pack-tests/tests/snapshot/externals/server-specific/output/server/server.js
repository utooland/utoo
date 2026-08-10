((__UTOOPACK__) => {
// Dummy runtime
})([
["server.js",

"[project]/externals/server-specific/input/node_modules/client-only/index.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "default",
    ()=>__TURBOPACK__default__export__
]);
var __TURBOPACK__default__export__ = "bundled client-only";
}),
"[externals]/server-only [external] (server-only, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("server-only", () => require("server-only"));

module.exports = mod;
}),
"[project]/externals/server-specific/input/server.ts [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([]);
var __TURBOPACK__imported__module__$5b$project$5d2f$externals$2f$server$2d$specific$2f$input$2f$node_modules$2f$client$2d$only$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/externals/server-specific/input/node_modules/client-only/index.js [server] (ecmascript)");
var __TURBOPACK__imported__module__$5b$externals$5d2f$server$2d$only__$5b$external$5d$__$28$server$2d$only$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/server-only [external] (server-only, cjs)");
;
;
console.log(__TURBOPACK__imported__module__$5b$project$5d2f$externals$2f$server$2d$specific$2f$input$2f$node_modules$2f$client$2d$only$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["default"], __TURBOPACK__imported__module__$5b$externals$5d2f$server$2d$only__$5b$external$5d$__$28$server$2d$only$2c$__cjs$29$__["default"]);
}),
],
["server.js", {"otherChunks":[],"runtimeModuleIds":["[project]/externals/server-specific/input/server.ts [server] (ecmascript)"]}],
]);