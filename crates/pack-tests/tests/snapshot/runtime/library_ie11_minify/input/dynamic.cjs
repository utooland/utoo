module.exports = Object.create({ inherited: 5 });
Object.defineProperty(module.exports, "hidden", { value: 6 });
Object.defineProperty(module.exports, "computed", {
  enumerable: true,
  get() {
    return this.dynamicValue;
  },
});
module.exports.default = "excluded";
module.exports.staticValue = "from CommonJS";
module.exports[globalThis.dynamicKey] = 17;
