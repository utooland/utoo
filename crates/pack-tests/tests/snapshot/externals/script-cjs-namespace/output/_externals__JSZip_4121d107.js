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

if (mod && mod.__esModule) {
  __turbopack_context__.n(mod);
} else {
  var ns = Object.create(null);
  var isEsmNamespace = mod && typeof Symbol !== 'undefined' && Symbol.toStringTag && mod[Symbol.toStringTag] === 'Module';
  if (mod && (typeof mod === 'object' || typeof mod === 'function')) {
    for (var key in mod) ns[key] = mod[key];
  }
  if (!isEsmNamespace) ns.default = mod;
  Object.defineProperty(ns, '__esModule', { value: true });
  if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(ns, Symbol.toStringTag, { value: 'Module' });
  }
  __turbopack_context__.n(ns);
}
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
]);