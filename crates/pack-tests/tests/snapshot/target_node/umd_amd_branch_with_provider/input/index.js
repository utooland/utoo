module.exports = (function (definition) {
  if (typeof define === "function") {
    return define(["./amd-dep"], definition);
  } else if (typeof module !== "undefined" && module.exports) {
    return definition(require("./cjs-dep"));
  }
})(function (value) {
  return value;
});
