module.exports = [
"[externals]/node:path [external] (node:path, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("node:path", () => require("node:path"));

module.exports = mod;
}),
"[project]/target_node/umd_amd_branch/input/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = function(definition) {
    if ("TURBOPACK compile-time falsy", 0) //TURBOPACK unreachable
    ;
    else if (("TURBOPACK compile-time value", "object") !== "undefined" && module.exports) {
        return definition(__turbopack_context__.r("[externals]/node:path [external] (node:path, cjs)"));
    }
}(function(path) {
    return path.basename("/tmp/utoo");
});
}),
];

//# sourceMappingURL=_root-of-the-server___1e31a885.js.map