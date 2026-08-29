(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/dynamic_import/very-dynamic/input/index.ts [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

async function importUrl(url) {
    return Promise.resolve().then(()=>{
        const e = new Error("Cannot find module as expression is too dynamic");
        e.code = 'MODULE_NOT_FOUND';
        throw e;
    }).then((module)=>module.default);
}
async function requireStat(name) {
    return (()=>{
        const e = new Error("Cannot find module as expression is too dynamic");
        e.code = 'MODULE_NOT_FOUND';
        throw e;
    })();
}
__turbopack_context__.s([
    "importUrl",
    0,
    importUrl,
    "requireStat",
    0,
    requireStat
]);
}),
]);

//# sourceMappingURL=input_index_ts_0hglm01kujxmh.js.map