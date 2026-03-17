((__UTOOPACK__) => {
// Dummy runtime
})([
["main.js",

"[externals]/fs [external] (fs, cjs)", (function(__turbopack_context__, module, exports) {

const mod = __turbopack_context__.x("fs", () => require("fs"));

module.exports = mod;
}),
"[project]/target_node/library_build_node_target/input/helper.js [library-server] (ecmascript)", (function(__turbopack_context__, module, exports) {

const fs = __turbopack_context__.r("[externals]/fs [external] (fs, cjs)");
function readFile(filePath) {
    return fs.readFileSync(filePath, "utf-8");
}
module.exports = {
    readFile
};
}),
"[externals]/path [external] (path, cjs)", (function(__turbopack_context__, module, exports) {

const mod = __turbopack_context__.x("path", () => require("path"));

module.exports = mod;
}),
"[project]/target_node/library_build_node_target/input/index.js [library-server] (ecmascript)", (function(__turbopack_context__, module, exports) {

const path = __turbopack_context__.r("[externals]/path [external] (path, cjs)");
function getFullPath(name) {
    return path.join(("TURBOPACK compile-time value", "/ROOT/target_node/library_build_node_target/input"), name);
}
async function loadHelper() {
    const helper = await Promise.resolve().then(()=>__turbopack_context__.i("[project]/target_node/library_build_node_target/input/helper.js [library-server] (ecmascript)"));
    return helper.readFile;
}
module.exports = {
    getFullPath,
    loadHelper
};
}),
],
["main.js", {"otherChunks":[],"runtimeModuleIds":["[project]/target_node/library_build_node_target/input/index.js [library-server] (ecmascript)"]}],
]);


//# sourceMappingURL=main.js.map