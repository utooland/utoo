module.exports = (function (definition) {
  if (typeof define === "function") {
    define(["missing-amd-only"], definition);
  } else if (typeof module !== "undefined" && module.exports) {
    return definition(require("node:path"));
  }
})(function (path) {
  return path.basename("/tmp/utoo");
});
