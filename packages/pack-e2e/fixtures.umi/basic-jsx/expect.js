const assert = require("node:assert/strict");

module.exports = ({ files }) => {
  const allText = Object.values(files).join("\n");
  assert.match(allText, /basic jsx fixture/);
};
