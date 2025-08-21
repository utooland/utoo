import { build } from "./build";
import { serve } from "./dev";
import * as webpackCompat from "./webpackCompat";

export { build };
export { serve };

const utoopack = { build, serve };
export default utoopack;
export * from "./find-root";
export * from "./types";
export * from "./webpackCompat";
export type WebpackConfig = webpackCompat.WebpackConfig;
namespace utoopack {
  export type WebpackConfig = webpackCompat.WebpackConfig;
}
