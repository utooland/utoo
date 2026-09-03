(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/concatenate_modules/export-local-name-conflict/input/index.js [client] (ecmascript)", ((__turbopack_context__) => {
"use strict";

// MERGED MODULE: [project]/concatenate_modules/export-local-name-conflict/input/index.js [client] (ecmascript)
;
// MERGED MODULE: [project]/concatenate_modules/export-local-name-conflict/input/site-runtime.js [client] (ecmascript)
;
// MERGED MODULE: [project]/concatenate_modules/export-local-name-conflict/input/config.js [client] (ecmascript)
;
const appConfig = {
    name: 'from-config',
    appId: '123'
};
// MERGED MODULE: [project]/concatenate_modules/export-local-name-conflict/input/sdk.js [client] (ecmascript)
;
;
function transformProxyUrl(originPath) {
    var ternAppConfig = appConfig;
    return ternAppConfig.name + ':' + originPath;
}
function getTernIndexAppProps() {
    var yuyanId = appConfig.yuyanId, appName = appConfig.name;
    return appName + ':' + yuyanId;
}
function unusedTernExport() {
    return 'unused';
}
;
;
appConfig;
function getTernIndexAppProps1() {
    return getTernIndexAppProps();
}
// MERGED MODULE: [project]/concatenate_modules/export-local-name-conflict/input/micro-app.js [client] (ecmascript)
;
const appConfig1 = {
    name: 'from-local'
};
function readLocalName() {
    return appConfig1.name;
}
;
;
console.log(getTernIndexAppProps1(), readLocalName());
__turbopack_context__.s([], "[project]/concatenate_modules/export-local-name-conflict/input/index.js [client] (ecmascript)");
}),
]);

//# sourceMappingURL=input_index_1xpah8f408lg4.js.map