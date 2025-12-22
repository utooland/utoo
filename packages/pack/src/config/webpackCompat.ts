import {
  type BundleOptions,
  compatOptionsFromWebpack,
  type WebpackConfig,
} from "@utoo/pack-shared";
import { readWebpackConfig } from "./readWebpackConfig";

export {
  compatOptionsFromWebpack,
  type WebpackConfig,
} from "@utoo/pack-shared";
export { readWebpackConfig } from "./readWebpackConfig";

export function resolveBundleOptions(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
): BundleOptions {
  if ((<WebpackConfig>options).webpackMode) {
    let webpackConfig = <WebpackConfig>options;
    const loadedConfig = readWebpackConfig(projectPath, rootPath);
    webpackConfig = { ...loadedConfig, ...webpackConfig };
    try {
      return compatOptionsFromWebpack(webpackConfig);
    } catch (e) {
      throw new Error("Error converting webpack config: " + e);
    }
  } else {
    return <BundleOptions>options;
  }
}
