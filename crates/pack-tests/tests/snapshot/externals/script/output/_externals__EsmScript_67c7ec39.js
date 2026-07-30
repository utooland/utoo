(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/EsmScript [external] (EsmScript@https://example.com/esm-script.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["EsmScript"] !== 'undefined') {
    return globalThis["EsmScript"];
  }
  await __turbopack_context__.S("https://example.com/esm-script.js");
  if (typeof globalThis["EsmScript"] !== 'undefined') {
    return globalThis["EsmScript"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/esm-script.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/esm-script.js";
  throw error;
})();

__turbopack_context__.n(__turbopack_context__.N(mod));
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
]);