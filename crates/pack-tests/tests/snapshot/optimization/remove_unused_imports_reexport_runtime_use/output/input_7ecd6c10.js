(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 0>", ((__turbopack_context__) => {
"use strict";

;
;
__turbopack_context__.s([]);
}),
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/visibility.js [client] (ecmascript) <internal part 0>", ((__turbopack_context__) => {
"use strict";

function visibility(node, visible) {
    node.visible = visible;
}
;
;
__turbopack_context__.s([
    "visibility",
    0,
    visibility
]);
}),
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 4>", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 0>");
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$node_modules$2f$pkg$2f$visibility$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/visibility.js [client] (ecmascript) <internal part 0>");
;
;
function applyVisibility(node) {
    (0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$node_modules$2f$pkg$2f$visibility$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__["visibility"])(node, true);
    return node.visible;
}
;
;
__turbopack_context__.s([
    "applyVisibility",
    0,
    applyVisibility
]);
}),
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/selection.js [client] (ecmascript) <internal part 0>", ((__turbopack_context__) => {
"use strict";

function select(node) {
    return {
        append (name) {
            const child = {
                name
            };
            node.children.push(child);
            return child;
        }
    };
}
;
;
__turbopack_context__.s([
    "select",
    0,
    select
]);
}),
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 3>", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 0>");
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$node_modules$2f$pkg$2f$selection$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/selection.js [client] (ecmascript) <internal part 0>");
;
;
function renderTicks(node) {
    return (0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$node_modules$2f$pkg$2f$selection$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__["select"])(node).append('tick').name;
}
;
;
__turbopack_context__.s([
    "renderTicks",
    0,
    renderTicks
]);
}),
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__0$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 0>");
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__4$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 4>");
var __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__3$3e$__ = __turbopack_context__.i("[project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript) <internal part 3>");
;
const node = {
    children: [],
    visible: false
};
console.log((0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__3$3e$__["renderTicks"])(node), (0, __TURBOPACK__imported__module__$5b$project$5d2f$optimization$2f$remove_unused_imports_reexport_runtime_use$2f$input$2f$component$2e$js__$5b$client$5d$__$28$ecmascript$29$__$3c$internal__part__4$3e$__["applyVisibility"])(node));
__turbopack_context__.s([]);
}),
]);

//# sourceMappingURL=input_7ecd6c10.js.map