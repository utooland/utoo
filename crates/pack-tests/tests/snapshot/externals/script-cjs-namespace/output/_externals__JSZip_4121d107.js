(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/JSZip [external] (JSZip@https://example.com/jszip.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["JSZip"] !== 'undefined') {
    return globalThis["JSZip"];
  }
  await __turbopack_context__.S("https://example.com/jszip.js");
  if (typeof globalThis["JSZip"] !== 'undefined') {
    return globalThis["JSZip"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/jszip.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/jszip.js";
  throw error;
})();

__turbopack_context__.n(__turbopack_context__.N(mod));
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
]);