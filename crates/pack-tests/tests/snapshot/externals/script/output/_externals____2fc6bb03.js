(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
20, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

let mod;
try {
  await __turbopack_context__.L("https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js");
  if (typeof global["_"] === 'undefined') {
    throw new Error('Variable "_" is not available on global object after loading "https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js"');
  }
  mod = global["_"];
} catch (error) {
  throw new Error('Failed to load external URL module "_@https://gw.alipayobjects.com/os/lib/lodash/4.17.21/lodash.min.js": ' + (error.message || error));
}

__turbopack_context__.v(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
]);