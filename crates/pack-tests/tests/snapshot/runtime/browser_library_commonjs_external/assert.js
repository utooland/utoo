const assert = require("node:assert");

const library = require("./output/main.js");

assert.strictEqual(library.default, "external value");
