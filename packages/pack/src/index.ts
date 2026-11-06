import { analyze } from "./commands/analyze";
import { build } from "./commands/build";
import { serve } from "./commands/dev";
import * as webpackCompat from "./config/webpackCompat";

export { build };
export { analyze };
export type { AnalyzeOptions, AnalyzeResult } from "./commands/analyze";
export type { AnalyzeServer } from "./commands/serveAnalyzeReport";
export { serveAnalyzeReport } from "./commands/serveAnalyzeReport";
export { serve };
export type {
  DevServerClientBuildContext,
  DevServerHandle,
  DevServerReadyContext,
  StartServerOptions,
} from "./commands/dev";
export { defineConfig } from "./defineConfig";

const utoopack = { build, serve, analyze };
export default utoopack;
export * from "./config/types";
export * from "./config/webpackCompat";
export * from "./core/types";
export {
  BaseUpdate,
  EcmascriptMergedUpdate,
  IssuesUpdate,
  PartialUpdate,
  Update,
} from "./core/types";
export * from "./utils/findRoot";
export type WebpackConfig = webpackCompat.WebpackConfig;
namespace utoopack {
  export type WebpackConfig = webpackCompat.WebpackConfig;
}
