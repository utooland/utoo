(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript, server reference)", (function(__turbopack_context__){

}),
"[project]/basic/server_function/input/transport.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function createServerReference(id, name) {
    return async function(...args) {
        console.log(`[Transport] Call ${name} (${id}) with`, args);
        return {
            ok: true
        };
    };
}
__turbopack_context__.s([
    "createServerReference",
    0,
    createServerReference
]);
}),
"[project]/basic/server_function/input/actions.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$server$2d$fn$5d$__$28$ecmascript$2c$__server__reference$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript, server reference)");
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/transport.ts [client] (ecmascript)");
;
;
const createUser = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["createServerReference"])("bdadcaefd8ce9058", "createUser");
const deleteUser = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["createServerReference"])("63c89b1b411a2fd0", "deleteUser");
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

//# sourceMappingURL=input_019d5048.js.map