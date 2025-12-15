
exports.loadConfig = function loadConfig(path) {
  if (typeof self.__systemjs_require__ === 'function') {
    try {
      const config = self.__systemjs_require__(path);
      const resolvedConfig = config && config.default ? config.default : config;
      if (resolvedConfig && resolvedConfig.important === undefined) {
        resolvedConfig.important = true;
      }
      return resolvedConfig;
    } catch (e) {
      console.error("Worker: Failed to load tailwind config:", e);
      
      let animatePlugin = {};
      try {
        animatePlugin = self.__systemjs_require__('tailwindcss-animate');
      } catch (e) {}

      return {
        important: true,
        content: [],
        theme: { extend: {} },
        plugins: [animatePlugin],
      };
    }
  }
    console.error("Worker: Failed to load tailwind config: typeof self.__systemjs_require__  is not function");

  return {
        content: [],
        theme: { extend: {} },
        plugins: [],
      };
};

exports.useCustomJiti = function() {};
