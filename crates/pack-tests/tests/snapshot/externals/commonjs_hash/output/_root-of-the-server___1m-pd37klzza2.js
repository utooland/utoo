module.exports = [
"[externals]/hash-package/string/#/contains [external] (hash-package/string/#/contains, cjs)", ((__turbopack_context__, module, exports) => {

var mod = __turbopack_context__.x("hash-package/string/#/contains", () => require("hash-package/string/#/contains"));

module.exports = mod;
}),
"[project]/externals/commonjs_hash/input/index.js [server] (ecmascript)", ((__turbopack_context__, module, exports) => {

const contains = __turbopack_context__.r("[externals]/hash-package/string/#/contains [external] (hash-package/string/#/contains, cjs)");
module.exports = {
    contains: contains.call("hello", "ell"),
    missing: contains.call("hello", "world"),
    filename: contains.filename
};
}),
];