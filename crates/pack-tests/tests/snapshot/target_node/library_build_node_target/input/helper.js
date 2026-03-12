const fs = require("fs");

function readFile(filePath) {
  return fs.readFileSync(filePath, "utf-8");
}

module.exports = { readFile };
