(globalThis.TURBOPACK = globalThis.TURBOPACK || []).push(["main.js", {

16: ((__turbopack_context__) => {
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
11: ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("react-dom", () => require("react-dom")); } else { mod = globalThis["ReactDOM"] }

__turbopack_context__.v(mod);
}),
86: ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s({
    "a": ()=>a
});
const a = "aaa";
}),
27: ((__turbopack_context__) => {
"use strict";

__turbopack_context__.s({});
var __TURBOPACK__imported__module__16__ = __turbopack_context__.i(16);
// @ts-ignore
var __TURBOPACK__imported__module__11__ = __turbopack_context__.i(11);
var __TURBOPACK__imported__module__86__ = __turbopack_context__.i(86);
;
console.log('hello here');
;
;
console.log(__TURBOPACK__imported__module__86__["a"]);
function App({ content }) {
    // @ts-ignore
    return /*#__PURE__*/ (0, __TURBOPACK__imported__module__16__["jsx"])("div", {
        children: content
    });
}
// @ts-ignore
const root = __TURBOPACK__imported__module__11__["default"].createRoot(document.getElementById('root'));
root.render(/*#__PURE__*/ (0, __TURBOPACK__imported__module__16__["jsx"])(App, {
    content: 'hello'
}));
}),
},
{"otherChunks":[],"runtimeModuleIds":[27]},
]);
// Dummy runtime


//# sourceMappingURL=main.js.map