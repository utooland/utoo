import value from "./greeting.js" with { turbopackLoader: "./inline-loader.cjs" };
globalThis.value = value;
