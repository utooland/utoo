const path = require("path");

function getFullPath(name) {
  return path.join(__dirname, name);
}

async function loadHelper() {
  const helper = await import("./helper.js");
  return helper.readFile;
}

module.exports = { getFullPath, loadHelper };

