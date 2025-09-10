// @ts-ignore
import AppTsx from "./demo_raw/App.tsx";
// @ts-ignore
import IndexTsx from "./demo_raw/index.tsx";
// @ts-ignore
import PackageJson from "./demo_raw/package.json";
// @ts-ignore
import PackageLock from "./demo_raw/package-lock.json";
// @ts-ignore
import UtoopackJson from "./demo_raw/utoopack.json";

export const demoFiles: Record<string, any> = {
  "src/index.tsx": IndexTsx,
  "src/App.tsx": AppTsx,
  "utoopack.json": UtoopackJson,
  "package.json": PackageJson,
  "package-lock.json": PackageLock,
};
