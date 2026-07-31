function define(_dependencies, factory) {
  return factory("amd");
}

define.amd = {};
module.exports = define;
