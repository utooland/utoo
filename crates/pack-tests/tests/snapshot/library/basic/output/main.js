((__UTOOPACK__) => {
// Dummy runtime
})([
["main.js",

61, ((__turbopack_context__) => {
"use strict";

function jsx() {
    return 'purposefully empty stub for @emotion/react/jsx-runtime.js';
}
function jsxs() {
    return 'purposefully empty stub for @emotion/react/jsx-runtime.js';
}
__turbopack_context__.s([
    "jsx",
    0,
    jsx
]);
}),
11, ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("react-dom", () => require("react-dom")); } else { mod = globalThis["ReactDOM"] }

__turbopack_context__.v(mod);
}),
32, ((__turbopack_context__) => {
"use strict";

let mod; if (typeof exports === 'object' && typeof module === 'object') { mod = __turbopack_context__.x("emotion-styled", () => require("emotion-styled")); } else { mod = globalThis["EmotionStyled"] }

__turbopack_context__.v(mod);
}),
81, ((__turbopack_context__) => {
"use strict";

const a = "aaa";
__turbopack_context__.s([
    "a",
    0,
    a
]);
}),
20, ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__61__ = __turbopack_context__.i(61);
// @ts-ignore
var __TURBOPACK__imported__module__11__ = __turbopack_context__.i(11);
var __TURBOPACK__imported__module__32__ = __turbopack_context__.i(32);
var __TURBOPACK__imported__module__81__ = __turbopack_context__.i(81);
;
/** @jsxImportSource @emotion/react */ console.log('hello here');
;
;
console.log(__TURBOPACK__imported__module__32__["styled"]);
;
console.log(__TURBOPACK__imported__module__81__["a"]);
function App({ content }) {
    // @ts-ignore
    return /*#__PURE__*/ (0, __TURBOPACK__imported__module__61__["jsx"])("div", {
        children: content
    });
}
// @ts-ignore
const root = __TURBOPACK__imported__module__11__["default"].createRoot(document.getElementById('root'));
root.render(/*#__PURE__*/ (0, __TURBOPACK__imported__module__61__["jsx"])(App, {
    content: 'hello'
}));
__turbopack_context__.s([]);
}),
],
["main.js", {"otherChunks":[],"runtimeModuleIds":[20]}],
]);


//# sourceMappingURL=main.js.map