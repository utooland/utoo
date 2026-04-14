(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript, server reference)", (function(__turbopack_context__){

}),
"[project]/basic/server_function/input/actions.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$server$2d$fn$5d$__$28$ecmascript$2c$__server__reference$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript, server reference)");
(()=>{
    const e = new Error("Cannot find module '@app/transport'");
    e.code = 'MODULE_NOT_FOUND';
    throw e;
})();
;
;
const createUser = (...args)=>callServer("actions.ts#createUser", args);
const deleteUser = (...args)=>callServer("actions.ts#deleteUser", args);
__turbopack_context__.s([
    "createUser",
    0,
    createUser,
    "deleteUser",
    0,
    deleteUser
]);
}),
"[project]/basic/server_function/input/index.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/actions.ts [client] (ecmascript)");
;
async function main() {
    const user = await (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["createUser"])("Alice", "alice@example.com");
    console.log("Created user:", user);
    await (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["deleteUser"])(user.id);
    console.log("Deleted user");
}
main();
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_8e9d07ec.js.map