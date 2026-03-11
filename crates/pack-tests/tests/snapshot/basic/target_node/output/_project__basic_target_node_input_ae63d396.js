module.exports = [
"[project]/basic/target_node/input/utils.ts [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

function greet(name) {
    return `Hello, ${name}!`;
}
function add(a, b) {
    return a + b;
}
__turbopack_context__.s([
    "greet",
    0,
    greet
]);
}),
"[project]/basic/target_node/input/index.ts [server] (ecmascript)", ((__turbopack_context__) => {
"use strict";

var __TURBOPACK__url__external__node$3a$path__ = __turbopack_context__.x("node:path", ()=>require("node:path"), true);
var __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$target_node$2f$input$2f$utils$2e$ts__$5b$server$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/basic/target_node/input/utils.ts [server] (ecmascript)");
;
;
const name = process.env.USER ?? "world";
const message = (0, __TURBOPACK__imported__module__$5b$project$5d2f$basic$2f$target_node$2f$input$2f$utils$2e$ts__$5b$server$5d$__$28$ecmascript$29$__["greet"])(name);
console.log(message);
console.log("cwd:", process.cwd());
console.log("dirname:", __TURBOPACK__url__external__node$3a$path__["default"].dirname("/foo/bar/baz.txt"));
console.log("extname:", __TURBOPACK__url__external__node$3a$path__["default"].extname("index.ts"));
console.log("joined:", __TURBOPACK__url__external__node$3a$path__["default"].join("src", "utils", "index.ts"));
__turbopack_context__.s([]);
}),
];

//# sourceMappingURL=_project__basic_target_node_input_ae63d396.js.map