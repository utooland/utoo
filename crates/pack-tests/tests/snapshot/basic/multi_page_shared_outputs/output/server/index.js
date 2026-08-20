((__UTOOPACK__) => {
// Dummy runtime
})([
["index.js",

"[project]/basic/multi_page_shared_outputs/input/register.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "registerServerReference",
    ()=>registerServerReference
]);
function registerServerReference(action, id, name) {
    globalThis.serverActions ??= new Map();
    globalThis.serverActions.set(`${id}:${name}`, action);
}
}),
"[project]/basic/multi_page_shared_outputs/input/actions.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "pageBAction",
    ()=>pageBAction
]);
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$multi_page_shared_outputs$2f$input$2f$register$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/multi_page_shared_outputs/input/register.js [server] (ecmascript)");
"use server";
async function pageBAction() {
    return "page-b";
}
;
(0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$multi_page_shared_outputs$2f$input$2f$register$2e$js__$5b$server$5d$__$28$ecmascript$29$__["registerServerReference"])(pageBAction, "ebfb242f4750b77b", "pageBAction");
}),
"[project]/basic/multi_page_shared_outputs/input/server.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

console.log("server");
}),
],
["index.js", {"otherChunks":[],"runtimeModuleIds":["[project]/basic/multi_page_shared_outputs/input/actions.js [server] (ecmascript)","[project]/basic/multi_page_shared_outputs/input/server.js [server] (ecmascript)"]}],
]);


//# sourceMappingURL=index.js.map