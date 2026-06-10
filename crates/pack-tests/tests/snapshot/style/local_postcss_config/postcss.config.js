module.exports = {
  plugins: [
    [
      "postcss-plugin-px2rem",
      {
        rootValue: 16,
        unitPrecision: 5,
        replace: true,
        mediaQuery: false,
        minPixelValue: 0
      }
    ]
  ]
};
