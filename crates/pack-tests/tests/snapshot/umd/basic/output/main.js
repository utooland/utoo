((__TURBOPACK__) => {
// Dummy runtime
})([["main.js", {

16: ((__turbopack_context__) => {
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
    jsx
]);
}),
11: ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("react-dom", () => require("react-dom")); } else { mod = globalThis["ReactDOM"] }

__turbopack_context__.v(mod);
}),
32: ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("emotion-styled", () => require("emotion-styled")); } else { mod = globalThis["EmotionStyled"] }

__turbopack_context__.v(mod);
}),
86: ((__turbopack_context__) => {
"use strict";

const a = "aaa";
__turbopack_context__.s([
    "a",
    0,
    a
]);
}),
27: ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__16__ = __turbopack_context__.i(16);
// @ts-ignore
var __TURBOPACK__imported__module__11__ = __turbopack_context__.i(11);
var __TURBOPACK__imported__module__32__ = __turbopack_context__.i(32);
var __TURBOPACK__imported__module__86__ = __turbopack_context__.i(86);
;
console.log('hello here');
;
;
console.log(__TURBOPACK__imported__module__32__["styled"]);
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
__turbopack_context__.s([]);
}),
},
{"otherChunks":[],"runtimeModuleIds":[27]},
]]);


//# sourceMappingURL=main.js.map