((__UTOOPACK__) => {
// Dummy runtime
})([
["index.js",

"[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "createUser",
    ()=>createUser,
    "deleteUser",
    ()=>deleteUser
]);
"use server";
async function createUser(name, email) {
    // This runs on the server
    const user = {
        id: Math.random().toString(36),
        name,
        email
    };
    return user;
}
async function deleteUser(id) {
    // This runs on the server
    console.log(`Deleting user ${id}`);
}
}),
],
["index.js", {"otherChunks":[],"runtimeModuleIds":["[project]/basic/server_function/input/actions.ts [server-fn] (ecmascript)"]}],
]);


//# sourceMappingURL=index.js.map