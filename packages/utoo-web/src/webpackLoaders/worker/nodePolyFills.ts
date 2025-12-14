const assert = require("assert");
const buffer = require("buffer");
const constants = require("constants");
const path = require("path");
const url = require("url");
const util = require("util");
const less = require("less/lib/less-node/index.js").default;
const lessLoader = require("../loaders/less-loader");
import * as fs from "./fsPolyfill";

export default {
  assert,
  buffer,
  constants,
  fs,
  path,
  url,
  util,
  less,
  "less-loader": lessLoader,
};
