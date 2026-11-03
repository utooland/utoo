(globalThis.TURBOPACK||(globalThis.TURBOPACK=[])).push(["object"==typeof document?document.currentScript:void 0,"[project]/dynamic_import/very-dynamic/input/index.ts [client] (ecmascript)",((__turbopack_context__) => {
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
})]);

//# sourceMappingURL=0zn5byhe7r9bc.js.map