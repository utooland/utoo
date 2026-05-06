(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/optimization/remove_unused_imports_reexport_runtime_use/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

// MERGED MODULE: [project]/optimization/remove_unused_imports_reexport_runtime_use/input/index.js [client] (ecmascript)
;
// MERGED MODULE: [project]/optimization/remove_unused_imports_reexport_runtime_use/input/component.js [client] (ecmascript)
;
// MERGED MODULE: [project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/selection.js [client] (ecmascript)
;
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
// MERGED MODULE: [project]/optimization/remove_unused_imports_reexport_runtime_use/input/node_modules/pkg/visibility.js [client] (ecmascript)
;
function visibility(node, visible) {
    node.visible = visible;
}
;
function renderTicks(node) {
    return select(node).append('tick').name;
}
function applyVisibility(node) {
    visibility(node, true);
    return node.visible;
}
;
const node = {
    children: [],
    visible: false
};
console.log(renderTicks(node), applyVisibility(node));
__turbopack_context__.s([], "[project]/optimization/remove_unused_imports_reexport_runtime_use/input/index.js [client] (ecmascript)");
}),
]);

//# sourceMappingURL=input_index_9fa2ebb7.js.map