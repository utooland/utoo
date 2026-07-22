const staticPath = require("node:path");

export function staticBasename(value) {
  return staticPath.basename(value);
}

export function load(request) {
  return require(request);
}

export function loadAgain(request) {
  return require(request);
}
