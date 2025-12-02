"use strict";

Object.defineProperty(exports, "__esModule", {
  value: true
});
exports.default = void 0;
var _path = _interopRequireDefault(require("path"));
var _options = _interopRequireDefault(require("./options.json"));
var _utils = require("./utils");
function _interopRequireDefault(e) { return e && e.__esModule ? e : { default: e }; }
async function lessLoader(source) {
  const options = this.getOptions(_options.default);
  const callback = this.async();
  let implementation;
  try {
    implementation = (0, _utils.getLessImplementation)(this, options.implementation);
  } catch (error) {
    callback(error);
    return;
  }
  if (!implementation) {
    callback(new Error(`The Less implementation "${options.implementation}" not found`));
    return;
  }
  const lessOptions = (0, _utils.getLessOptions)(this, options, implementation);
  const useSourceMap = typeof options.sourceMap === "boolean" ? options.sourceMap : this.sourceMap;
  if (useSourceMap) {
    lessOptions.sourceMap = {
      outputSourceFiles: true
    };
  }
  let data = source;
  if (typeof options.additionalData !== "undefined") {
    data = typeof options.additionalData === "function" ? `${await options.additionalData(data, this)}` : `${options.additionalData}\n${data}`;
  }
  const logger = this.getLogger("less-loader");
  const loaderContext = this;
  const loggerListener = {
    error(message) {
      // TODO enable by default in the next major release
      if (options.lessLogAsWarnOrErr) {
        loaderContext.emitError(new Error(message));
      } else {
        logger.error(message);
      }
    },
    warn(message) {
      // TODO enable by default in the next major release
      if (options.lessLogAsWarnOrErr) {
        loaderContext.emitWarning(new Error(message));
      } else {
        logger.warn(message);
      }
    },
    info(message) {
      logger.log(message);
    },
    debug(message) {
      logger.debug(message);
    }
  };
  implementation.logger.addListener(loggerListener);
  let result;
  try {
    result = await implementation.render(data, lessOptions);
  } catch (error) {
    if (error.filename) {
      // `less` returns forward slashes on windows when `webpack` resolver return an absolute windows path in `WebpackFileManager`
      // Ref: https://github.com/webpack-contrib/less-loader/issues/357
      this.addDependency(_path.default.normalize(error.filename));
    }
    callback((0, _utils.errorFactory)(error));
    return;
  } finally {
    // Fix memory leaks in `less`
    implementation.logger.removeListener(loggerListener);
    delete lessOptions.pluginManager.webpackLoaderContext;
    delete lessOptions.pluginManager;
  }
  const {
    css,
    imports
  } = result;
  imports.forEach(item => {
    if ((0, _utils.isUnsupportedUrl)(item)) {
      return;
    }

    // `less` return forward slashes on windows when `webpack` resolver return an absolute windows path in `WebpackFileManager`
    // Ref: https://github.com/webpack-contrib/less-loader/issues/357
    const normalizedItem = _path.default.normalize(item);

    // Custom `importer` can return only `contents` so item will be relative
    if (_path.default.isAbsolute(normalizedItem)) {
      this.addDependency(normalizedItem);
    }
  });
  let map = typeof result.map === "string" ? JSON.parse(result.map) : result.map;
  if (map && useSourceMap) {
    map = (0, _utils.normalizeSourceMap)(map, this.rootContext);
  }
  callback(null, css, map);
}
var _default = exports.default = lessLoader;