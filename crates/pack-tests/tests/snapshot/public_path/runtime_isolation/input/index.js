globalThis.__runtimePublicPathTest = {
  loadFirst: () => import("./first.js"),
  loadSecond: () => import("./second.js"),
  setPublicPath(path) {
    __webpack_public_path__ = path;
  },
  getPublicPath() {
    return __webpack_public_path__;
  },
};
