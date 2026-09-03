(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/node_modules/jquery/index.js [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = function $(selector) {
    return {
        selector
    };
};
}),
"[project]/node_modules/buffer-dep/index.js [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

exports.Buffer = {
    from: function(str) {
        return {
            data: str
        };
    }
};
}),
"[project]/provider/basic/input/index.js [client] (ecmascript)", ((__turbopack_context__, module, exports) => {

var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$jquery$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = /*#__PURE__*/ __turbopack_context__.i("[project]/node_modules/jquery/index.js [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$buffer$2d$dep$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = /*#__PURE__*/ __turbopack_context__.i("[project]/node_modules/buffer-dep/index.js [client] (ecmascript)");
// Test ProvidePlugin functionality
// $ should be automatically imported from 'jquery'
// Buffer should be automatically imported from 'buffer'
console.log((0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$jquery$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__["default"])('#app'));
console.log(__TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$buffer$2d$dep$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__["Buffer"].from('hello'));
}),
]);

//# sourceMappingURL=_project___1r0ck5j7wce6f.js.map