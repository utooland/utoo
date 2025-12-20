import path from "path";

export function readWebpackConfig(projectPath?: string, rootPath?: string) {
  const projectPathOutOfRoot =
    projectPath === undefined
      ? process.cwd()
      : path.join(rootPath ?? "", projectPath);
  const configPath = path.join(projectPathOutOfRoot, "webpack.config.js");
  return require(configPath);
}
