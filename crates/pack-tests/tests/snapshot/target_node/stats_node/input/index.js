const { getBaseName } = require("./utils");

function processPath(filePath) {
  return getBaseName(filePath);
}

module.exports = { processPath };
