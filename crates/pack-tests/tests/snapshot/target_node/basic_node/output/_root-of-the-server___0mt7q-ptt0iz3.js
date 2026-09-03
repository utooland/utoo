module.exports = [
"[externals]/node:path [external] (node:path, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("node:path", () => require("node:path"));

module.exports = mod;
}),
"[project]/target_node/basic_node/input/utils.ts [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function greet(name) {
    return `Hello, ${name}!`;
}
function add(a, b) {
    return a + b;
}
__turbopack_context__.s([
    "f",
    0,
    greet
]);
}),
"[project]/target_node/basic_node/input/index.ts [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$path__$5b$external$5d$__$28$node$3a$path$2c$__cjs$29$__ = __turbopack_context__.i("[externals]/node:path [external] (node:path, cjs)");
var __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$basic_node$2f$input$2f$utils$2e$ts__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_node/basic_node/input/utils.ts [server] (ecmascript)");
;
;
const name = process.env.USER ?? "world";
const message = (0, __TURBOPACK__imported__module__$5b$project$5d2f$target_node$2f$basic_node$2f$input$2f$utils$2e$ts__$5b$server$5d$__$28$ecmascript$29$__["f"])(name);
console.log(message);
console.log("cwd:", process.cwd());
console.log("dirname:", __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$path__$5b$external$5d$__$28$node$3a$path$2c$__cjs$29$__["default"].dirname("/foo/bar/baz.txt"));
console.log("extname:", __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$path__$5b$external$5d$__$28$node$3a$path$2c$__cjs$29$__["default"].extname("index.ts"));
console.log("joined:", __TURBOPACK__imported__module__$5b$externals$5d2f$node$3a$path__$5b$external$5d$__$28$node$3a$path$2c$__cjs$29$__["default"].join("src", "utils", "index.ts"));
__turbopack_context__.s([]);
}),
];

//# sourceMappingURL=_root-of-the-server___0mt7q-ptt0iz3.js.map