const assert = require("node:assert/strict");
const { Greeting } = require("./output/main.js");
assert.equal(new Greeting().greet("Ada"), "Server greeting for Ada");
