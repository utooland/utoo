(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/basic/server_function/input/actions.ts [server] (ecmascript, server reference)", (function(__turbopack_context__){

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
    "f",
    0,
    createServerReference
]);
}),
"[project]/basic/server_function/input/actions.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$server$5d$__$28$ecmascript$2c$__server__reference$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/actions.ts [server] (ecmascript, server reference)");
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/transport.ts [client] (ecmascript)");
;
;
const createUser = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["f"])("bdadcaefd8ce9058", "createUser");
const deleteUser = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["f"])("63c89b1b411a2fd0", "deleteUser");
__turbopack_context__.s([
    "F",
    0,
    createUser,
    "M",
    0,
    deleteUser
]);
}),
"[project]/basic/server_function/input/admin.ts [server] (ecmascript, server reference)", (function(__turbopack_context__){

}),
"[project]/basic/server_function/input/admin.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$admin$2e$ts__$5b$server$5d$__$28$ecmascript$2c$__server__reference$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/admin.ts [server] (ecmascript, server reference)");
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/transport.ts [client] (ecmascript)");
;
;
const createUser = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$transport$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["f"])("18fccc60e484024c", "createUser");
__turbopack_context__.s([
    "f",
    0,
    createUser
]);
}),
"[project]/basic/server_function/input/index.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/actions.ts [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$admin$2e$ts__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/server_function/input/admin.ts [client] (ecmascript)");
;
;
async function main() {
    const user = await (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["F"])("Alice", "alice@example.com");
    console.log("Created user:", user);
    await (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$actions$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["M"])(user.id);
    console.log("Deleted user");
    const admin = await (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$server_function$2f$input$2f$admin$2e$ts__$5b$client$5d$__$28$ecmascript$29$__["f"])("Bob", "superadmin");
    console.log("Created admin user:", admin);
}
main();
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_1oleh6oy595fs.js.map