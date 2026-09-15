(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
(()=>{"use strict";return[
"[project]/basic/react_compiler_react17/input/index.jsx [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/react/jsx-runtime.js [client] (ecmascript)");
var __TURBOPACK__imported__module__$5b$externals$5d2f$react$2d$compiler$2d$runtime__$5b$external$5d$__$28$react$2d$compiler$2d$runtime$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/react-compiler-runtime [external] (react-compiler-runtime, cjs)");
var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/react/index.js [client] (ecmascript)");
;
;
;
function Counter(t0) {
    const $ = (0, __TURBOPACK__imported__module__$5b$externals$5d2f$react$2d$compiler$2d$runtime__$5b$external$5d$__$28$react$2d$compiler$2d$runtime$2c$__cjs$29$__["c"])(10);
    const { initialCount } = t0;
    const [count, setCount] = (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$index$2e$js__$5b$client$5d$__$28$ecmascript$29$__["f"])(initialCount);
    const doubled = count * 2;
    let t1;
    if ($[0] !== count) {
        t1 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["s"])("p", {
            children: count
        });
        $[0] = count;
        $[1] = t1;
    } else {
        t1 = $[1];
    }
    let t2;
    if ($[2] !== doubled) {
        t2 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["s"])("p", {
            children: doubled
        });
        $[2] = doubled;
        $[3] = t2;
    } else {
        t2 = $[3];
    }
    let t3;
    if ($[4] !== count) {
        t3 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["s"])("button", {
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
        t4 = /*#__PURE__*/ (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f$react$2f$jsx$2d$runtime$2e$js__$5b$client$5d$__$28$ecmascript$29$__["j"])("div", {
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
    "f",
    0,
    useState
]);
}),
"[project]/node_modules/react/jsx-runtime.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function jsx() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
function jsxs() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
__turbopack_context__.s([
    "s",
    0,
    jsx,
    "j",
    0,
    jsxs
]);
}),
]})(),
"[externals]/react-compiler-runtime [external] (react-compiler-runtime, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("react-compiler-runtime", () => require("react-compiler-runtime"));

module.exports = mod;
}),
]);

//# sourceMappingURL=2w9g5yyx92lni.js.map