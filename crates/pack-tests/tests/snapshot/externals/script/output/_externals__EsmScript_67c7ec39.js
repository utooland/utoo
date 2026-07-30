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

if (mod && mod.__esModule) {
  __turbopack_context__.n(mod);
} else {
  var ns = Object.create(null);
  if (mod && (typeof mod === 'object' || typeof mod === 'function')) {
    for (var key in mod) ns[key] = mod[key];
  }
  ns.default = mod;
  __turbopack_context__.n(ns);
}
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
]);