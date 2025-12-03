// @ts-ignore
import AppLess from "./demo_raw/App.less";
// @ts-ignore
import AppTsx from "./demo_raw/App.tsx";
// @ts-ignore
import IndexCss from "./demo_raw/index.css";
// @ts-ignore
import IndexTsx from "./demo_raw/index.tsx";
// @ts-ignore
import PackageJson from "./demo_raw/package.json";
// @ts-ignore
import PackageLock from "./demo_raw/package-lock.json";
// @ts-ignore
import PostCssConfig from "./demo_raw/postcss.config.json";
// @ts-ignore
import TailwindConfig from "./demo_raw/tailwind.config.json";
// @ts-ignore
import UtoopackJson from "./demo_raw/utoopack.json";

export const demoFiles: Record<string, any> = {
  "src/index.tsx": IndexTsx,
  "src/index.css": IndexCss,
  "src/App.tsx": AppTsx,
  "src/App.less": AppLess,
  "postcss.config.json": PostCssConfig,
  "tailwind.config.json": TailwindConfig,
  "utoopack.json": UtoopackJson,
  "package.json": PackageJson,
  "package-lock.json": PackageLock,
};
