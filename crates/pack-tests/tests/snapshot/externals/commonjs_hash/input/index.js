const contains = require("hash-package/string/#/contains");

module.exports = {
  contains: contains.call("hello", "ell"),
  missing: contains.call("hello", "world"),
  filename: contains.filename,
};
