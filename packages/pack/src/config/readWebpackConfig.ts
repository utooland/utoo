import fs from "fs";
import path from "path";

export function readWebpackConfig(projectPath?: string, rootPath?: string) {
  const projectPathOutOfRoot =
    projectPath === undefined
      ? process.cwd()
      : path.join(rootPath ?? "", projectPath);

  const { env, argv } = parseArgs();

  try {
    const configPath = path.resolve(projectPathOutOfRoot, "./webpack.config");
    const configFile = [".js", ".cjs"]
      .map((ext) => configPath + ext)
      .find(fs.existsSync);

    const config = require(configFile || configPath);
    if (typeof config === "function") {
      return config(env, argv);
    }
    return config;
  } catch (error) {
    if (error && (error as { code?: string }).code === "MODULE_NOT_FOUND") {
      throw new Error(
        `Webpack config not found in "${projectPathOutOfRoot}". Make sure a webpack configuration file (e.g., webpack.config.js) exists when using the --webpack flag.`,
      );
    }
    throw error;
  }
}

function parseArgs() {
  const env: Record<string, any> = {};
  const argv: Record<string, any> = {};

  process.argv.forEach((arg) => {
    if (arg.startsWith("--env")) {
      if (arg.startsWith("--env.")) {
        const [key, val] = arg.substring(6).split("=");
        env[key] = val === undefined ? true : val;
      } else if (arg === "--env") {
        // noop
      } else {
        const [key, val] = arg.substring(5).split("=");
        env[key] = val === undefined ? true : val;
      }
    }
    if (arg.startsWith("--")) {
      const [key, val] = arg.substring(2).split("=");
      argv[key] = val === undefined ? true : val;
    }
  });
  return { env, argv };
}
