module.exports = [
"[project]/target_node/cjs_esm_interop/input/node_modules/supports-color/index.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function createSupportsColor(stream) {
    return {
        level: stream && stream.isTTY ? 1 : 0
    };
}
const supportsColor = {
    stdout: createSupportsColor({
        isTTY: true
    }),
    stderr: createSupportsColor({
        isTTY: true
    })
};
var __TURBOPACK__default__export__ = supportsColor;
__turbopack_context__.s([
    "I",
    0,
    createSupportsColor,
    "U",
    0,
    __TURBOPACK__default__export__
]);
}),
"[project]/target_node/cjs_esm_interop/input/node_modules/json5/index.cjs [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = {
    parse (source) {
        const match = /feature:'([^']+)'/.exec(source);
        return {
            feature: match ? match[1] : "unknown"
        };
    }
};
}),
"[project]/target_node/cjs_esm_interop/input/index.js [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$supports$2d$color$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_node/cjs_esm_interop/input/node_modules/supports-color/index.js [server] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$json5$2f$index$2e$cjs__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_node/cjs_esm_interop/input/node_modules/json5/index.cjs [server] (ecmascript)");
;
;
const stdout = (0, __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$supports$2d$color$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["I"])({
    isTTY: true
});
const parsed = __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$json5$2f$index$2e$cjs__$5b$server$5d$__$28$ecmascript$29$__["default"].parse("{feature:'interop'}");
console.log("supports-color", typeof __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$supports$2d$color$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["I"], __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$cjs_esm_interop$2f$input$2f$node_modules$2f$supports$2d$color$2f$index$2e$js__$5b$server$5d$__$28$ecmascript$29$__["U"].stdout.level);
console.log("supports-color stdout", stdout.level);
console.log("json5 default", parsed.feature);
__turbopack_context__.s([]);
}),
];

//# sourceMappingURL=_project__target_node_cjs_esm_interop_input_08_5ex79p1a9u.js.map