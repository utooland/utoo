// @ts-ignore
const context = require.context(
  "./demo_raw",
  true,
  /\.(js|jsx|ts|tsx|css|less|scss|sass|json|html|vue|svg|png|jpg|jpeg|gif)$/,
);

export const demoFiles: Record<string, any> = {};

context.keys().forEach((key: string) => {
  const fileName = key.replace(/^\.\//, "");
  if (
    !/\.(js|jsx|ts|tsx|css|less|scss|sass|json|html|vue|svg|png|jpg|jpeg|gif)$/.test(
      fileName,
    )
  )
    return;
  demoFiles[fileName] = context(key);
});
