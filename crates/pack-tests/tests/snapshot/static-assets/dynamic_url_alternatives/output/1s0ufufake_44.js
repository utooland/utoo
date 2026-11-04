(globalThis.TURBOPACK||(globalThis.TURBOPACK=[])).push(["object"==typeof document?document.currentScript:void 0,"[project]/static-assets/dynamic_url_alternatives/input/fallback.txt (static in ecmascript)",((__turbopack_context__) => {

__turbopack_context__.q("/fallback.81eb51f5.txt");}),"[project]/static-assets/dynamic_url_alternatives/input/index.js [client] (ecmascript)",((__turbopack_context__, module, exports) => {

var __TURBOPACK__import$2e$meta__ = {
    get url () {
        return __turbopack_context__.F("input/index.js");
    },
    env: {
        DEV: false,
        PROD: true,
        MODE: "production",
        BASE_URL: "/",
        SSR: false
    }
};
globalThis.createWorker = ({ classWorkerURL, ...options } = {})=>{
    if (classWorkerURL) {
        return new Worker(new URL(classWorkerURL, __TURBOPACK__import$2e$meta__.url), options);
    }
};
globalThis.fallbackURL = (override)=>override != null ? new URL(override, __TURBOPACK__import$2e$meta__.url) : new __turbopack_context__.U(__turbopack_context__.r("[project]/static-assets/dynamic_url_alternatives/input/fallback.txt (static in ecmascript)"));
})]);

//# sourceMappingURL=1s0ufufake_44.js.map