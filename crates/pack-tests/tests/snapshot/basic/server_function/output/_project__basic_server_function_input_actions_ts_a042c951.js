module.exports = [
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
];

//# sourceMappingURL=_project__basic_server_function_input_actions_ts_a042c951.js.map