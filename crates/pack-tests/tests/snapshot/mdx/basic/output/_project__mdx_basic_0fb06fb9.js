(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/mdx/basic/node_modules/react/jsx-runtime.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

const Fragment = "Fragment";
function jsx(type, props) {
    return {
        type,
        props
    };
}
function jsxs(type, props) {
    return {
        type,
        props
    };
}
__turbopack_context__.s([
    "Fragment",
    0,
    Fragment,
    "jsx",
    0,
    jsx,
    "jsxs",
    0,
    jsxs
]);
}),
"[project]/mdx/basic/input/content.mdx.tsx [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/mdx/basic/node_modules/react/jsx-runtime.js [client] (ecmascript)");
;
function _createMdxContent(props) {
    const _components = Object.assign({
        h1: "h1",
        p: "p"
    }, props.components);
    return (0, __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsxs"])(__TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["Fragment"], {
        children: [
            (0, __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])(_components.h1, {
                children: "Hello MDX"
            }),
            "\n",
            (0, __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])(_components.p, {
                children: "This content was compiled by utoopack."
            })
        ]
    });
}
function MDXContent(props = {}) {
    const { wrapper: MDXLayout } = props.components || {};
    return MDXLayout ? (0, __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])(MDXLayout, Object.assign({}, props, {
        children: (0, __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])(_createMdxContent, props)
    })) : _createMdxContent(props);
}
var __TURBOPACK__default__export__ = MDXContent;
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
}),
"[project]/mdx/basic/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$input$2f$content$2e$mdx$2e$tsx__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/mdx/basic/input/content.mdx.tsx [client] (ecmascript)");
;
var __TURBOPACK__default__export__ = __TURBOPACK__imported__module__$5b$project$5d2f$mdx$2f$basic$2f$input$2f$content$2e$mdx$2e$tsx__$5b$client$5d$__$28$ecmascript$29$__["default"];
__turbopack_context__.s([
    "default",
    0,
    __TURBOPACK__default__export__
]);
}),
]);

//# sourceMappingURL=_project__mdx_basic_0fb06fb9.js.map