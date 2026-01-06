(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/optimization/remove_unused_imports_reexport/input/wrapper.js [client] (ecmascript) <locals>", ((__turbopack_context__) => {
"use strict";

// Imports fnA from pkg and USES it
// Re-exports fnB from pkg
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport$2f$input$2f$node_modules$2f$pkg$2f$a$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport/input/node_modules/pkg/a.js [client] (ecmascript)");
;
;
function useFnA() {
    return (0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport$2f$input$2f$node_modules$2f$pkg$2f$a$2e$js__$5b$client$5d$__$28$ecmascript$29$__["fnA"])();
}
__turbopack_context__.s([]);
}),
"[project]/optimization/remove_unused_imports_reexport/input/node_modules/pkg/b.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function fnB() {
    return 'B';
}
__turbopack_context__.s([
    "fnB",
    0,
    fnB
]);
}),
"[project]/optimization/remove_unused_imports_reexport/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

// Entry only imports fnB, but wrapper.js uses fnA
// BUG: fnA factory is removed because entry doesn't use it
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport$2f$input$2f$wrapper$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport/input/wrapper.js [client] (ecmascript) <locals>");
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport$2f$input$2f$node_modules$2f$pkg$2f$b$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport/input/node_modules/pkg/b.js [client] (ecmascript)");
;
console.log((0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport$2f$input$2f$node_modules$2f$pkg$2f$b$2e$js__$5b$client$5d$__$28$ecmascript$29$__["fnB"])());
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_121f988c.js.map