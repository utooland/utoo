import micromatch from "micromatch";
import * as path from "path";
import * as fs from "./fsPolyfill";

const walkSync = (currentDir: string, rootDir: string, entries: string[]) => {
  let list: any[];
  try {
    list = fs.readdirSync(currentDir, { withFileTypes: true });
  } catch (e) {
    return;
  }

  for (const entry of list) {
    const fullPath = path.join(currentDir, entry.name);
    const relativePath = path.relative(rootDir, fullPath);

    if (entry.isDirectory()) {
      walkSync(fullPath, rootDir, entries);
    } else if (entry.isFile()) {
      entries.push(relativePath);
    }
  }
};

const fastGlob: any = (patterns: string | string[], options: any = {}) => {
  return Promise.resolve(fastGlob.sync(patterns, options));
};

fastGlob.sync = (patterns: string | string[], options: any = {}) => {
  const cwd = options.cwd || "/";
  const ignore = options.ignore || [];
  const allFiles: string[] = [];

  walkSync(cwd, cwd, allFiles);

  const matched = micromatch(allFiles, patterns, {
    ignore: ignore,
    dot: options.dot,
    cwd: cwd,
  });

  if (options.absolute) {
    return matched.map((p) => path.join(cwd, p));
  }

  return matched;
};

fastGlob.stream = (patterns: string | string[], options: any = {}) => {
  throw new Error("fastGlob.stream is not implemented in polyfill");
};

fastGlob.async = fastGlob;
fastGlob.generateTasks = () => [];
fastGlob.isDynamicPattern = (p: string) => micromatch.scan(p).isGlob;
fastGlob.escapePath = (p: string) => p.replace(/([*?|(){}[\]])/g, "\\$1");

export default fastGlob;
