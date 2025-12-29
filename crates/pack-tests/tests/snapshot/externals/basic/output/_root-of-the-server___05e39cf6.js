(globalThis.TURBOPACK || (globalThis.TURBOPACK = [])).push([typeof document === "object" ? document.currentScript : undefined,
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
298, ((__turbopack_context__) => {
"use strict";

const mod = __turbopack_context__.x("bar", () => require("bar"));

__turbopack_context__.n(mod);
}),
227, ((__turbopack_context__) => {
"use strict";

const mod = __turbopack_context__.x("bar_import2", () => require("bar_import2"));

__turbopack_context__.n(mod);
}),
462, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

let mod;
try {
  await __turbopack_context__.L("https://example.com/lib/script.js");
  if (typeof global["bar_script1"] === 'undefined') {
    throw new Error('Variable "bar_script1" is not available on global object after loading "https://example.com/lib/script.js"');
  }
  mod = global["bar_script1"];
} catch (error) {
  throw new Error('Failed to load external URL module "bar_script1@https://example.com/lib/script.js": ' + (error.message || error));
}

__turbopack_context__.v(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
747, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

let mod;
try {
  await __turbopack_context__.L("https://example.com/lib/script2.js");
  if (typeof global["bar_script2"] === 'undefined') {
    throw new Error('Variable "bar_script2" is not available on global object after loading "https://example.com/lib/script2.js"');
  }
  mod = global["bar_script2"];
} catch (error) {
  throw new Error('Failed to load external URL module "bar_script2@https://example.com/lib/script2.js": ' + (error.message || error));
}

__turbopack_context__.v(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
212, ((__turbopack_context__) => {
"use strict";

return __turbopack_context__.a(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {

var __TURBOPACK__imported__module__631__ = __turbopack_context__.i(631);
var __TURBOPACK__imported__module__414__ = __turbopack_context__.i(414);
var __TURBOPACK__imported__module__43__ = __turbopack_context__.i(43);
var __TURBOPACK__imported__module__298__ = __turbopack_context__.i(298);
var __TURBOPACK__imported__module__227__ = __turbopack_context__.i(227);
var __TURBOPACK__imported__module__462__ = __turbopack_context__.i(462);
var __TURBOPACK__imported__module__747__ = __turbopack_context__.i(747);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__462__,
    __TURBOPACK__imported__module__747__
]);
[__TURBOPACK__imported__module__462__, __TURBOPACK__imported__module__747__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
;
;
;
;
;
;
console.log(__TURBOPACK__imported__module__631__["default"], __TURBOPACK__imported__module__414__["default"], __TURBOPACK__imported__module__43__["default"], __TURBOPACK__imported__module__298__["default"], __TURBOPACK__imported__module__227__["default"]);
console.log(__TURBOPACK__imported__module__462__["default"], __TURBOPACK__imported__module__747__["default"]);
__turbopack_context__.s([]);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
]);

//# sourceMappingURL=_root-of-the-server___05e39cf6.js.map