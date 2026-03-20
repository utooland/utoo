// @ts-check
import { defineConfig } from "@utoo/pack";

export default defineConfig({
  entry: [
    {
      name: "index",
      import: "./src/index.jsx",
      html: {
        template: "./index.html",
      },
    },
  ],
  output: {
    clean: true,
  },
  optimization: {
    minify: false,
  },
  styles: {
    postcss: {
      plugins: [
        [
          "postcss-plugin-px2rem",
          {
            rootValue: 16,
            unitPrecision: 5,
            replace: true,
            mediaQuery: false,
            minPixelValue: 0,
          },
        ],
      ],
    },
  },
});
