(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
12, ((__turbopack_context__) => {
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
    ()=>importUrl,
    "requireStat",
    ()=>requireStat
]);
}),
]);

//# sourceMappingURL=dynamic_import_input_index_ts_403d7278.js.map