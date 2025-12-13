// @ts-ignore
const context = require.context("./demo_raw", true, /\.\w+$/);

export const demoFiles: Record<string, any> = {};

context.keys().forEach((key: string) => {
  const fileName = key.replace(/^\.\//, "");
  if (!/\.\w+$/.test(fileName)) return;
  demoFiles[key] = context(key);
});
