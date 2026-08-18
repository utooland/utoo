const queue = {
  delete(value) {
    return value;
  },
};

globalThis.__arrowFunctionTargetQueue = queue;

import("./lazy.js").then(function (module) {
  globalThis.__arrowFunctionTargetValue = module.value;
});
