((__UTOOPACK__) => {
// Dummy runtime
})([
["index.js",

"[project]/basic/server_function/input/register.ts [server-fn] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "RUNTIME_ACTIONS",
    ()=>RUNTIME_ACTIONS,
    "registerServerReference",
    ()=>registerServerReference
]);
const RUNTIME_ACTIONS = new Map();
function registerServerReference(action, id, name) {
    console.log(`[Register] Action ${name} (${id}) registered.`);
    RUNTIME_ACTIONS.set(id, action);
}
}),
"[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "createUser",
    ()=>createUser1,
    "deleteUser",
    ()=>deleteUser1
]);
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$register$2e$ts__$5b$server$2d$fn$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/register.ts [server-fn] (ecmascript)");
"use server";
async function createUser1(name, email) {
    // This runs on the server
    const user = {
        id: Math.random().toString(36),
        name,
        email
    };
    return user;
}
async function deleteUser1(id) {
    // This runs on the server
    console.log(`Deleting user ${id}`);
}
;
(0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$register$2e$ts__$5b$server$2d$fn$5d$__$28$ecmascript$29$__["registerServerReference"])(createUser, "bdadcaefd8ce9058", "createUser");
(0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$register$2e$ts__$5b$server$2d$fn$5d$__$28$ecmascript$29$__["registerServerReference"])(deleteUser, "63c89b1b411a2fd0", "deleteUser");
}),
"[project]/basic/server_function/input/server.ts [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

console.log("This is the main Server Entry");
}),
],
["index.js", {"otherChunks":[],"runtimeModuleIds":["[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript)","[project]/basic/server_function/input/server.ts [server] (ecmascript)"]}],
]);


//# sourceMappingURL=index.js.map