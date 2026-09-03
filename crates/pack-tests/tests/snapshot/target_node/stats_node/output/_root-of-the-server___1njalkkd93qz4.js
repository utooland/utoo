module.exports = [
"[externals]/path [external] (path, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("path", () => require("path"));

module.exports = mod;
}),
"[project]/target_node/stats_node/input/utils.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

const path = __turbopack_context__.r("[externals]/path [external] (path, cjs)");
function getBaseName(filePath) {
    return path.basename(filePath);
}
module.exports = {
    getBaseName
};
}),
"[project]/target_node/stats_node/input/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

const { getBaseName } = __turbopack_context__.r("[project]/target_node/stats_node/input/utils.js [server] (ecmascript)");
function processPath(filePath) {
    return getBaseName(filePath);
}
module.exports = {
    processPath
};
}),
];

//# sourceMappingURL=_root-of-the-server___1njalkkd93qz4.js.map