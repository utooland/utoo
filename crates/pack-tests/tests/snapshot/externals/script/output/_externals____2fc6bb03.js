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

__turbopack_context__.n(__turbopack_context__.N(mod));
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); }
}, true);
}),
]);