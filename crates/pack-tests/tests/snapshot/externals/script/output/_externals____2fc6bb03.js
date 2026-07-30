(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[externals]/_ [external] (_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js, script)", ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
try {
var mod = await (async () => {
  if (typeof globalThis["_"] !== 'undefined') {
    return globalThis["_"];
  }
  await __turbopack_context__.S("https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js");
  if (typeof globalThis["_"] !== 'undefined') {
    return globalThis["_"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js";
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