import "systemjs/dist/system.js";

import nodePolyFills from "./polyfills/nodePolyFills";

const fs = nodePolyFills.fs;
const path = nodePolyFills.path;
const installedModules: Record<string, { exports: any }> = {};

const statCache: Record<string, any> = {};
const pkgJsonCache: Record<string, any> = {};
const resolutionCache: Record<string, string> = {};
const searchPathsCache: Record<string, string[]> = {};

const statSync = (p: string) => {
  if (
    p.includes("node_modules") &&
    Object.prototype.hasOwnProperty.call(statCache, p)
  ) {
    if (statCache[p] === false) {
      throw new Error("ENOENT");
    }
    return statCache[p];
  }
  try {
    const res = fs.statSync(p);
    if (p.includes("node_modules")) statCache[p] = res;
    return res;
  } catch (e) {
    if (p.includes("node_modules")) statCache[p] = false;
    throw e;
  }
};

const existsSync = (p: string) => {
  try {
    statSync(p);
    return true;
  } catch (e) {
    return false;
  }
};

const executeModule = (
  moduleCode: string,
  moduleId: string,
  id: string,
  importMaps: Record<string, string>,
  entrypoint: string,
) => {
  if (installedModules[moduleId]) {
    return installedModules[moduleId].exports;
  }

  const context = path.dirname(moduleId);

  let finalExports = {};
  const moduleRequire = (childId: string) =>
    loadModule(childId, context, importMaps, entrypoint);
  moduleRequire.resolve = (request: string) => request;

  const module = { exports: finalExports, require: moduleRequire };
  if (moduleId.includes("node_modules")) {
    installedModules[moduleId] = module;
  }

  // Hack for entrypoint
  if (moduleId === entrypoint) {
    moduleCode = "self.Buffer = require('buffer').Buffer;" + moduleCode;
  }

  if (!moduleId.endsWith(".json")) {
    moduleCode = `// [Debug] Resolved: ${moduleId}\n` + moduleCode;
  }

  try {
    if (moduleId.endsWith(".json")) {
      finalExports = JSON.parse(moduleCode);
      module.exports = finalExports;
    } else {
      new Function(
        "require",
        "exports",
        "module",
        "__filename",
        "__dirname",
        "process",
        moduleCode,
      )(
        moduleRequire,
        module.exports,
        module,
        moduleId,
        context,
        nodePolyFills.process,
      );
      finalExports = module.exports;
    }
  } catch (e: any) {
    console.error(`Worker: Error executing module ${moduleId}:`, e);
    throw new Error(`Failed to load dependency ${moduleId}: ${e.message}`);
  }

  const originalWarn = console.warn;
  console.warn = (...args: any[]) => {
    const msg = args[0]?.toString() || "";
    if (msg.includes("(SystemJS Error#W3")) {
      return;
    }
    originalWarn.apply(console, args);
  };
  try {
    System.set(moduleId, { default: finalExports });
  } catch (e) {
    // ignore
  } finally {
    console.warn = originalWarn;
  }
  return finalExports;
};

const loadModule = (
  id: string,
  context: string,
  importMaps: Record<string, string>,
  entrypoint: string,
) => {
  const cacheKey = `${context}:${id}`;
  if (resolutionCache[cacheKey]) {
    const cachedId = resolutionCache[cacheKey];
    if (installedModules[cachedId]) {
      return installedModules[cachedId].exports;
    }
    // Fast path: if we know the resolved path, try to load it directly
    try {
      const moduleCode = fs.readFileSync(cachedId, "utf8") as string;
      return executeModule(moduleCode, cachedId, id, importMaps, entrypoint);
    } catch (e) {
      // If read fails, fall back to full resolution
    }
  }

  // 1. Resolve
  let resolvedId = id;
  if (id.startsWith(".")) {
    resolvedId = path.resolve(context, id);
  }

  // 2. Check Cache (SystemJS)
  let dependency = System.get(resolvedId);
  if (dependency) return dependency.default;

  if (id !== resolvedId) {
    dependency = System.get(id);
    if (dependency) return dependency.default;
  }

  // 3. Check Node Polyfills
  if (id in nodePolyFills) {
    // @ts-ignore
    return nodePolyFills[id];
  }
  if (resolvedId in nodePolyFills) {
    // @ts-ignore
    return nodePolyFills[resolvedId];
  }

  // 4. Check importMaps & FS
  let moduleCode = importMaps[resolvedId] || importMaps[id];
  let moduleId = importMaps[resolvedId] ? resolvedId : id;

  // Fallback: Try resolving from node_modules
  if (!moduleCode && !id.startsWith(".") && !id.startsWith("/")) {
    let searchPaths = searchPathsCache[context];
    if (!searchPaths) {
      searchPaths = [];
      let currentDir = context;
      while (true) {
        if (path.basename(currentDir) !== "node_modules") {
          const nodeModulesPath = path.join(currentDir, "node_modules");
          if (!searchPaths.includes(nodeModulesPath)) {
            searchPaths.push(nodeModulesPath);
          }
        }
        const parent = path.dirname(currentDir);
        if (parent === currentDir) break;
        currentDir = parent;
      }
      // Ensure cwd/node_modules is always included
      const cwd = nodePolyFills.process.cwd();
      const cwdNodeModules = path.join(cwd, "node_modules");
      if (!searchPaths.includes(cwdNodeModules)) {
        searchPaths.push(cwdNodeModules);
      }
      searchPathsCache[context] = searchPaths;
    }

    for (const nodeModulesDir of searchPaths) {
      const nodeModulesPath = path.join(nodeModulesDir, id);

      // Check package.json first
      const pkgJsonPath = path.join(nodeModulesPath, "package.json");
      let pkg;
      try {
        const content = fs.readFileSync(pkgJsonPath, "utf8") as string;
        pkg = JSON.parse(content);
        pkgJsonCache[pkgJsonPath] = pkg;
      } catch (e) {
        // ignore
      }

      if (pkg) {
        try {
          const mainField = pkg.main;
          if (mainField) {
            const candidates = [
              path.resolve(nodeModulesPath, mainField),
              path.resolve(nodeModulesPath, mainField) + ".js",
              path.resolve(nodeModulesPath, mainField) + ".json",
              path.resolve(nodeModulesPath, mainField, "index.js"),
            ];
            for (const candidate of candidates) {
              try {
                // Try reading directly to bypass potential statSync issues
                moduleCode = fs.readFileSync(candidate, "utf8") as string;
                resolvedId = candidate;
                moduleId = candidate;
                break;
              } catch (e) {
                // ignore
              }
            }
          }
        } catch (e) {
          // ignore
        }
      }

      if (!moduleCode) {
        const extensions = ["", ".js", ".json", "/index.js"];
        for (const ext of extensions) {
          const p = nodeModulesPath + ext;
          try {
            // Try reading directly
            moduleCode = fs.readFileSync(p, "utf8") as string;
            resolvedId = p;
            moduleId = p;
            break;
          } catch (e) {
            // ignore
          }
        }
      }

      if (moduleCode) break;
    }
  }

  // Fallback: Try resolving absolute path
  if (!moduleCode && id.startsWith("/")) {
    const extensions = ["", ".js", ".json", "/index.js"];
    for (const ext of extensions) {
      if (ext === "/index.js" && id.endsWith(".js")) continue;
      const p = id + ext;
      try {
        moduleCode = fs.readFileSync(p, "utf8") as string;
        resolvedId = p;
        moduleId = id;
        break;
      } catch (e) {
        // ignore
      }
    }
  }

  if (!moduleCode) {
    // Try extensions
    const extensions = ["", ".js", ".json", "/index.js"];
    for (const ext of extensions) {
      if (ext === "/index.js" && id.endsWith(".js")) continue;
      const p = resolvedId + ext;
      try {
        moduleCode = fs.readFileSync(p, "utf8") as string;
        resolvedId = p;
        moduleId = p;
        break;
      } catch (e) {
        // ignore
      }
    }
  }

  if (moduleCode) {
    const cacheKey = `${context}:${id}`;
    resolutionCache[cacheKey] = moduleId;
    return executeModule(moduleCode, moduleId, id, importMaps, entrypoint);
  }

  console.error(
    `Worker: Dependency ${id} (resolved: ${resolvedId}) not found. Context: ${context}`,
  );
  return {};
};

export async function cjs(
  entrypoint: string,
  importMaps: Record<string, string>,
) {
  // Clear caches to avoid stale data across runs
  for (const key in statCache) delete statCache[key];
  for (const key in pkgJsonCache) delete pkgJsonCache[key];
  for (const key in resolutionCache) delete resolutionCache[key];
  for (const key in searchPathsCache) delete searchPathsCache[key];

  await Promise.all(
    Object.entries(importMaps).map(async ([k, v]) => {
      if (v.startsWith("https://")) {
        try {
          const response = await fetch(v);
          if (response.ok) {
            importMaps[k] = await response.text();
          } else {
            console.error(
              `Failed to fetch loader '${k}' from ${v}: ${response.status} ${response.statusText}`,
            );
            delete importMaps[k];
          }
        } catch (error) {
          console.error(`Error fetching loader '${k}' from ${v}:`, error);
          delete importMaps[k];
        }
      }
    }),
  );

  // @ts-ignore
  // a hack for loader-runner resolving
  self.__systemjs_require__ = (id: string) =>
    loadModule(id, path.dirname(entrypoint), importMaps, entrypoint);
  // @ts-ignore
  self.__systemjs_require__.resolve = (request: string) => request;

  loadModule(entrypoint, path.dirname(entrypoint), importMaps, entrypoint);
}
