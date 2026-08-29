(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/circular_dependency/reexport_cycle_class/input/barrel.js [client] (ecmascript) <locals>", ((__turbopack_context__) => {
"use strict";

;
;
__turbopack_context__.s([]);
}),
"[project]/circular_dependency/reexport_cycle_class/input/base.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

class Base {
}
__turbopack_context__.s([
    "Base",
    0,
    Base
]);
}),
"[project]/circular_dependency/reexport_cycle_class/input/child.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "Child",
    ()=>Child
]);
var __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/barrel.js [client] (ecmascript)");
;
class Child extends __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__["Q"] {
}
}),
"[project]/circular_dependency/reexport_cycle_class/input/barrel.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s([
    "Q",
    ()=>(__TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$base$2e$js__$5b$client$5d$__$28$ecmascript$29$__ ?? __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/base.js [client] (ecmascript)"))["Base"],
    "x",
    ()=>(__TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$child$2e$js__$5b$client$5d$__$28$ecmascript$29$__ ?? __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/child.js [client] (ecmascript)"))["Child"]
]);
var __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$locals$3e$__ = __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/barrel.js [client] (ecmascript) <locals>");
var __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$base$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/base.js [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$child$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/child.js [client] (ecmascript)");
}),
"[project]/circular_dependency/reexport_cycle_class/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/circular_dependency/reexport_cycle_class/input/barrel.js [client] (ecmascript)");
;
console.log(new __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__["x"]() instanceof __TURBOPACK__imported__module__$5b$project$5d2f$circular_dependency$2f$reexport_cycle_class$2f$input$2f$barrel$2e$js__$5b$client$5d$__$28$ecmascript$29$__["x"]);
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_0-9g5nwc1co3i.js.map