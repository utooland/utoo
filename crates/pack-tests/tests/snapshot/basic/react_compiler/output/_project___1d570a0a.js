(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/node_modules/react/jsx-runtime.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function jsx() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
function jsxs() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
__turbopack_context__.s([
    "jsx",
    0,
    jsx,
    "jsxs",
    0,
    jsxs
]);
}),
"[project]/node_modules/react/compiler-runtime.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function c(size) {
    return new Array(size);
}
__turbopack_context__.s([
    "c",
    0,
    c
]);
}),
"[project]/node_modules/react/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function jsx() {
    return 'purposefully empty stub for react/index.js';
}
function useState(initialState) {
    return [
        initialState,
        ()=>{}
    ];
}
__turbopack_context__.s([
    "useState",
    0,
    useState
]);
}),
"[project]/basic/react_compiler/input/index.jsx [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/react/jsx-runtime.js [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$compiler$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/react/compiler-runtime.js [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/react/index.js [client] (ecmascript)");
;
;
;
function Counter(t0) {
    const $ = (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$compiler$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["c"])(10);
    const { initialCount } = t0;
    const [count, setCount] = (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__["useState"])(initialCount);
    const doubled = count * 2;
    let t1;
    if ($[0] !== count) {
        t1 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])("p", {
            children: count
        });
        $[0] = count;
        $[1] = t1;
    } else {
        t1 = $[1];
    }
    let t2;
    if ($[2] !== doubled) {
        t2 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])("p", {
            children: doubled
        });
        $[2] = doubled;
        $[3] = t2;
    } else {
        t2 = $[3];
    }
    let t3;
    if ($[4] !== count) {
        t3 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsx"])("button", {
            onClick: ()=>setCount(count + 1),
            children: "increment"
        });
        $[4] = count;
        $[5] = t3;
    } else {
        t3 = $[5];
    }
    let t4;
    if ($[6] !== t1 || $[7] !== t2 || $[8] !== t3) {
        t4 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["jsxs"])("div", {
            children: [
                t1,
                t2,
                t3
            ]
        });
        $[6] = t1;
        $[7] = t2;
        $[8] = t3;
        $[9] = t4;
    } else {
        t4 = $[9];
    }
    return t4;
}
console.log(Counter);
__turbopack_context__.s([
    "Counter",
    0,
    Counter
]);
}),
]);

//# sourceMappingURL=_project___1d570a0a.js.map