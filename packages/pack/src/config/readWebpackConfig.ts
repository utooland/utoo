import path from "path";

export function readWebpackConfig(projectPath?: string, rootPath?: string) {
  const projectPathOutOfRoot =
    projectPath === undefined
      ? process.cwd()
      : path.join(rootPath ?? "", projectPath);
  try {
    const configPath = require.resolve("webpack.config", {
      paths: [projectPathOutOfRoot]
    });
    return require(configPath);
  } catch (error) {
    if (error && (error as { code?: string }).code === 'MODULE_NOT_FOUND') {
      throw new Error(
        `Webpack config not found in "${projectPathOutOfRoot}". Make sure a webpack configuration file (e.g., webpack.config.js) exists when using the --webpack flag.`
      );
    }
    throw error;
  }
}
