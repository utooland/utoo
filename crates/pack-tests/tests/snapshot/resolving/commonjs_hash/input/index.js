const contains = require("hash-package/string/#/contains");

module.exports = {
  contains: contains.call("hello", "ell"),
  missing: contains.call("hello", "world"),
  fragment: require("hash-package/plain.js#ignored"),
  resolved: require.resolve("hash-package/string/#/contains"),
  fallbackResolved: require.resolve("hash-package/plain.js#ignored"),
};
