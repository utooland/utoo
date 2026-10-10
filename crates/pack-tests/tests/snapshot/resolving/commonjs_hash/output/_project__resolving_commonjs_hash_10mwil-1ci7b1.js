module.exports = [
"[project]/resolving/commonjs_hash/input/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

const contains = __turbopack_context__.r("[project]/resolving/commonjs_hash/node_modules/hash-package/string/#/contains/index.js [server] (ecmascript)");
module.exports = {
    contains: contains.call("hello", "ell"),
    missing: contains.call("hello", "world"),
    fragment: __turbopack_context__.r("[project]/resolving/commonjs_hash/node_modules/hash-package/plain.js#ignored [server] (ecmascript)"),
    resolved: "[project]/resolving/commonjs_hash/node_modules/hash-package/string/#/contains/index.js [server] (ecmascript)",
    fallbackResolved: "[project]/resolving/commonjs_hash/node_modules/hash-package/plain.js#ignored [server] (ecmascript)"
};
}),
"[project]/resolving/commonjs_hash/node_modules/hash-package/plain.js#ignored [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = "fallback";
}),
"[project]/resolving/commonjs_hash/node_modules/hash-package/string/#/contains/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

module.exports = function contains(value) {
    return String(this).includes(value);
};
}),
];