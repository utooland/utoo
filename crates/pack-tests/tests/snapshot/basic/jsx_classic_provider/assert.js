const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const verify = require("./assertions");

const filename = path.join(__dirname, "output/main.js");
const context = { URL };
vm.runInNewContext(fs.readFileSync(filename, "utf8"), context, { filename });
verify(context.ClassicJsxProvider);
