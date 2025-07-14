(globalThis.TURBOPACK = globalThis.TURBOPACK || []).push([typeof document === "object" ? document.currentScript : undefined, {

263: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["bar"];

__turbopack_context__.n(mod);
}),
902: ((__turbopack_context__) => {

var { m: module, e: exports } = __turbopack_context__;
{
const mod = __turbopack_context__.x("bar", () => require("bar"));

module.exports = mod;
}}),
163: ((__turbopack_context__) => {

var { m: module, e: exports } = __turbopack_context__;
{
const mod = __turbopack_context__.x("bar_require2", () => require("bar_require2"));

module.exports = mod;
}}),
337: ((__turbopack_context__) => {
"use strict";

var { a: __turbopack_async_module__ } = __turbopack_context__;
__turbopack_async_module__(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {
const mod = await __turbopack_context__.y("bar");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
449: ((__turbopack_context__) => {
"use strict";

var { a: __turbopack_async_module__ } = __turbopack_context__;
__turbopack_async_module__(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {
const mod = await __turbopack_context__.y("bar_import2");

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
813: ((__turbopack_context__) => {
"use strict";

var { a: __turbopack_async_module__ } = __turbopack_context__;
__turbopack_async_module__(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {
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

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
481: ((__turbopack_context__) => {
"use strict";

var { a: __turbopack_async_module__ } = __turbopack_context__;
__turbopack_async_module__(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {
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

__turbopack_context__.n(mod);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, true);}),
729: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["antd"];

__turbopack_context__.n(mod);
}),
506: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["antd"]["Button"];

__turbopack_context__.n(mod);
}),
563: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["antd"]["DatePicker"];

__turbopack_context__.n(mod);
}),
521: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["antd"]["Input/Group"];

__turbopack_context__.n(mod);
}),
291: ((__turbopack_context__) => {
"use strict";

const mod = globalThis["antd"]["version"];

__turbopack_context__.n(mod);
}),
140: ((__turbopack_context__) => {
"use strict";

var { a: __turbopack_async_module__ } = __turbopack_context__;
__turbopack_async_module__(async (__turbopack_handle_async_dependencies__, __turbopack_async_result__) => { try {
__turbopack_context__.s({});
var __TURBOPACK__imported__module__263__ = __turbopack_context__.i(263);
var __TURBOPACK__imported__module__902__ = __turbopack_context__.i(902);
var __TURBOPACK__imported__module__163__ = __turbopack_context__.i(163);
var __TURBOPACK__imported__module__337__ = __turbopack_context__.i(337);
var __TURBOPACK__imported__module__449__ = __turbopack_context__.i(449);
var __TURBOPACK__imported__module__813__ = __turbopack_context__.i(813);
var __TURBOPACK__imported__module__481__ = __turbopack_context__.i(481);
var __TURBOPACK__imported__module__729__ = __turbopack_context__.i(729);
var __TURBOPACK__imported__module__506__ = __turbopack_context__.i(506);
var __TURBOPACK__imported__module__563__ = __turbopack_context__.i(563);
var __TURBOPACK__imported__module__521__ = __turbopack_context__.i(521);
var __TURBOPACK__imported__module__291__ = __turbopack_context__.i(291);
var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
    __TURBOPACK__imported__module__337__,
    __TURBOPACK__imported__module__449__,
    __TURBOPACK__imported__module__813__,
    __TURBOPACK__imported__module__481__
]);
[__TURBOPACK__imported__module__337__, __TURBOPACK__imported__module__449__, __TURBOPACK__imported__module__813__, __TURBOPACK__imported__module__481__] = __turbopack_async_dependencies__.then ? (await __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
;
;
;
;
;
;
;
;
;
;
;
;
// import zh_CN from "antd/es/locale/zh_CN";
// import zh_TW from "antd/es/locale/zh_TW";
// import Style from 'antd/es/button/style';
console.log(__TURBOPACK__imported__module__263__["default"], __TURBOPACK__imported__module__902__["default"], __TURBOPACK__imported__module__163__["default"], __TURBOPACK__imported__module__337__["default"], __TURBOPACK__imported__module__449__["default"]);
console.log(__TURBOPACK__imported__module__813__["default"], __TURBOPACK__imported__module__481__["default"]);
console.log(__TURBOPACK__imported__module__729__["default"]);
console.log(__TURBOPACK__imported__module__506__["default"], __TURBOPACK__imported__module__563__["default"]);
console.log(__TURBOPACK__imported__module__521__["default"]);
console.log(__TURBOPACK__imported__module__291__["default"], zh_CN, zh_TW); // console.log(Style);
__turbopack_async_result__();
} catch(e) { __turbopack_async_result__(e); } }, false);}),
}]);

//# sourceMappingURL=__5bc833c2.js.map