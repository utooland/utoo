module.exports = [
"[project]/target_node/umd_amd_branch_with_provider/input/define.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

function define(_dependencies, factory) {
    return factory("amd");
}
define.amd = {};
module.exports = define;
}),
"[project]/target_node/umd_amd_branch_with_provider/input/cjs-dep.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = "cjs";
}),
"[project]/target_node/umd_amd_branch_with_provider/input/amd-dep.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = "amd";
}),
"[project]/target_node/umd_amd_branch_with_provider/input/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$umd_amd_branch_with_provider$2f$input$2f$define$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = /*#__PURE__*/ __turbopack_context__.i("[project]/target_node/umd_amd_branch_with_provider/input/define.js [server] (ecmascript)");
module.exports = function(definition) {
    if (typeof __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$umd_amd_branch_with_provider$2f$input$2f$define$2e$js__$5b$server$5d$__$28$ecmascript$29$__["default"] === "function") {
        return ((r)=>r !== undefined && __turbopack_context__.v(r))(definition(__turbopack_context__.r("[project]/target_node/umd_amd_branch_with_provider/input/amd-dep.js [server] (ecmascript)")));
    } else if (("TURBOPACK compile-time value", "object") !== "undefined" && module.exports) {
        return definition(__turbopack_context__.r("[project]/target_node/umd_amd_branch_with_provider/input/cjs-dep.js [server] (ecmascript)"));
    }
}(function(value) {
    return value;
});
}),
];

//# sourceMappingURL=_project__target_node_umd_amd_branch_with_provider_input_ca9a3ee7.js.map