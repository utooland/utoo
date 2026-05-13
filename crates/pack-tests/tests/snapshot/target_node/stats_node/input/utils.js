const path = require("path");

function getBaseName(filePath) {
  return path.basename(filePath);
}

module.exports = { getBaseName };
