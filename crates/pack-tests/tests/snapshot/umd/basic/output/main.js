(globalThis.TURBOPACK = globalThis.TURBOPACK || []).push(["main.js", {

52: ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s({
    "jsx": ()=>jsx,
    "jsxs": ()=>jsxs
});
function jsx() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
function jsxs() {
    return 'purposefully empty stub for react/jsx-runtime.js';
}
}),
15: ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("react-dom", () => require("react-dom")); } else { mod = globalThis["ReactDOM"] }

__turbopack_context__.n(mod);
}),
30: ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s({
    "a": ()=>a
});
const a = "aaa";
}),
67: ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s({});
var __TURBOPACK__imported__module__52__ = __turbopack_context__.i(52);
// @ts-ignore
var __TURBOPACK__imported__module__15__ = __turbopack_context__.i(15);
var __TURBOPACK__imported__module__30__ = __turbopack_context__.i(30);
;
console.log('hello here');
;
;
console.log(__TURBOPACK__imported__module__30__["a"]);
function App({ content }) {
    // @ts-ignore
    return /*#__PURE__*/ (0, __TURBOPACK__imported__module__52__["jsx"])("div", {
        children: content
    });
}
// @ts-ignore
const root = __TURBOPACK__imported__module__15__["default"].createRoot(document.getElementById('root'));
root.render(/*#__PURE__*/ (0, __TURBOPACK__imported__module__52__["jsx"])(App, {
    content: 'hello'
}));
}),
},
{"otherChunks":[],"runtimeModuleIds":[67]},
]);
// Dummy runtime


//# sourceMappingURL=main.js.map