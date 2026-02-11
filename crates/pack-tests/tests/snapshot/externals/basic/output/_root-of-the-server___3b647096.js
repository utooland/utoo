(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
631, ((__turbopack_context__) => {
"use strict";

const mod = globalThis["bar"];

__turbopack_context__.v(mod);
}),
414, ((__turbopack_context__, module, exports) => {

const mod = __turbopack_context__.x("bar", () => require("bar"));

module.exports = mod;
}),
43, ((__turbopack_context__, module, exports) => {

const mod = __turbopack_context__.x("bar_require2", () => require("bar_require2"));

module.exports = mod;
}),
377, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const mod = await __turbopack_context__.y("bar");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
569, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const mod = await __turbopack_context__.y("bar_import2");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
17, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const mod = await (async () => {
  if (typeof globalThis["bar_script1"] !== 'undefined') {
    return globalThis["bar_script1"];
  }
  await __turbopack_context__.S("https://example.com/lib/script.js");
  if (typeof globalThis["bar_script1"] !== 'undefined') {
    return globalThis["bar_script1"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/lib/script.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/lib/script.js";
  throw error;
})();

__turbopack_context__.v(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
596, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

const mod = await (async () => {
  if (typeof globalThis["bar_script2"] !== 'undefined') {
    return globalThis["bar_script2"];
  }
  await __turbopack_context__.S("https://example.com/lib/script2.js");
  if (typeof globalThis["bar_script2"] !== 'undefined') {
    return globalThis["bar_script2"];
  }
  const error = new Error('Loading script failed.\n(missing: "https://example.com/lib/script2.js")');
  error.name = 'ScriptExternalLoadError';
  error.type = 'missing';
  error.request = "https://example.com/lib/script2.js";
  throw error;
})();

__turbopack_context__.v(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
212, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__631__ = __turbopack_context__.i(631);
var __TURBOPACK__imported__module__414__ = __turbopack_context__.i(414);
var __TURBOPACK__imported__module__43__ = __turbopack_context__.i(43);
var __TURBOPACK__imported__module__377__ = __turbopack_context__.i(377);
var __TURBOPACK__imported__module__569__ = __turbopack_context__.i(569);
var __TURBOPACK__imported__module__17__ = __turbopack_context__.i(17);
var __TURBOPACK__imported__module__596__ = __turbopack_context__.i(596);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__377__,
    __TURBOPACK__imported__module__569__,
    __TURBOPACK__imported__module__17__,
    __TURBOPACK__imported__module__596__
]);
[__TURBOPACK__imported__module__377__, __TURBOPACK__imported__module__569__, __TURBOPACK__imported__module__17__, __TURBOPACK__imported__module__596__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
;
;
;
;
;
;
console.log(__TURBOPACK__imported__module__631__["default"], __TURBOPACK__imported__module__414__["default"], __TURBOPACK__imported__module__43__["default"], __TURBOPACK__imported__module__377__["default"], __TURBOPACK__imported__module__569__["default"]);
console.log(__TURBOPACK__imported__module__17__["default"], __TURBOPACK__imported__module__596__["default"]);
__turbopack_context__.s([]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
]);

//# sourceMappingURL=_root-of-the-server___3b647096.js.map