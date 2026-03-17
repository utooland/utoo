((__UTOOPACK__) => {
// Dummy runtime
})([
["main.js",

"[project]/dynamic_import/library-chunk/input/a.ts [library-client] (ecmascript)", (function(__turbopack_context__) {
"use strict";

const a = "a";
__turbopack_context__.s([
    "a",
    0,
    a
]);
}),
"[project]/dynamic_import/library-chunk/input/index.ts [library-client] (ecmascript)", (function(__turbopack_context__) {
"use strict";

async function Test() {
    const module = await Promise.resolve().then(()=>__turbopack_context__.i("[project]/dynamic_import/library-chunk/input/a.ts [library-client] (ecmascript)"));
    return module;
}
__turbopack_context__.s([
    "Test",
    0,
    Test
]);
}),
],
["main.js", {"otherChunks":[],"runtimeModuleIds":["[project]/dynamic_import/library-chunk/input/index.ts [library-client] (ecmascript)"]}],
]);


//# sourceMappingURL=main.js.map