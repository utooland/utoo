import "systemjs/dist/system.js";

import nodePolyFills from "./polyfills/nodePolyFills";

const fs = nodePolyFills.fs;
const path = nodePolyFills.path;
const installedModules: Record<string, { exports: any }> = {};

const pkgJsonCache: Record<string, any> = {};
const resolutionCache: Record<string, string> = {};
const searchPathsCache: Record<string, string[]> = {};

const RESOLUTION_EXTENSIONS = [
  "",
  ".js",
  ".cjs",
  ".json",
  "/index.js",
  "/index.cjs",
  "/index.json",
];

const tryReadFile = (p: string): string | null => {
  try {
    return fs.readFileSync(p, "utf8") as string;
  } catch {
    return null;
  }
};

const resolveWithExtensions = (
  p: string,
  skipIndexJsIfJs = false,
): { code: string; id: string } | null => {
  for (const ext of RESOLUTION_EXTENSIONS) {
    if (skipIndexJsIfJs && ext === "/index.js" && p.endsWith(".js")) continue;
    const candidate = p + ext;
    const code = tryReadFile(candidate);
    if (code !== null) return { code, id: candidate };
  }
  return null;
};

const executeModule = (
  moduleId: string,
  moduleCode: string,
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
  if (moduleId.includes("node_modules") || moduleId in importMaps) {
    installedModules[moduleId] = module;
  }

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

  try {
    System.set(moduleId, { default: finalExports });
  } catch {
    // ignore
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
    if (installedModules[cachedId]) return installedModules[cachedId].exports;
  }

  // 1. Resolve
  let resolvedId = id.startsWith(".") ? path.join(context, id) : id;

  // 2. Check Cache (SystemJS)
  const sysDef =
    System.get(resolvedId) || (id !== resolvedId && System.get(id));
  if (sysDef) return sysDef.default;

  // 3. Check Node Polyfills
  if (id in nodePolyFills) return (nodePolyFills as any)[id];
  if (resolvedId in nodePolyFills) return (nodePolyFills as any)[resolvedId];

  // 4. Check importMaps
  let moduleCode = importMaps[id];
  let moduleId = moduleCode ? id : resolvedId;

  // Fallback: Try resolving from node_modules
  if (!moduleCode && !id.startsWith(".") && !id.startsWith("/")) {
    let searchPaths = searchPathsCache[context];
    if (!searchPaths) {
      searchPaths = [];
      let currentDir = context;
      while (true) {
        if (path.basename(currentDir) !== "node_modules") {
          const nmPath = path.join(currentDir, "node_modules");
          if (!searchPaths.includes(nmPath)) searchPaths.push(nmPath);
        }
        const parent = path.dirname(currentDir);
        if (parent === currentDir) break;
        currentDir = parent;
      }
      const cwdNm = path.join(nodePolyFills.process.cwd(), "node_modules");
      if (!searchPaths.includes(cwdNm)) searchPaths.push(cwdNm);
      searchPathsCache[context] = searchPaths;
    }

    for (const nodeModulesDir of searchPaths) {
      const nodeModulesPath = path.join(nodeModulesDir, id);
      const pkgPath = path.join(nodeModulesPath, "package.json");

      // Check package.json main
      let pkgMain;
      if (!pkgJsonCache[pkgPath]) {
        const json = tryReadFile(pkgPath);
        if (json) {
          try {
            pkgJsonCache[pkgPath] = JSON.parse(json);
          } catch {}
        }
      }
      if (pkgJsonCache[pkgPath]?.main) {
        pkgMain = path.resolve(nodeModulesPath, pkgJsonCache[pkgPath].main);
        const res = resolveWithExtensions(pkgMain);
        if (res) {
          moduleCode = res.code;
          moduleId = res.id;
          break;
        }
      }

      // Check direct file/directory
      const res = resolveWithExtensions(nodeModulesPath);
      if (res) {
        moduleCode = res.code;
        moduleId = res.id;
        break;
      }
    }
  }

  // Fallback: Try resolving absolute or relative path
  if (!moduleCode && (id.startsWith("/") || id.startsWith("."))) {
    const res = resolveWithExtensions(resolvedId, id.endsWith(".js"));
    if (res) {
      moduleCode = res.code;
      moduleId = res.id;
    }
  }

  if (moduleCode) {
    resolutionCache[`${context}:${id}`] = moduleId;
    return executeModule(moduleId, moduleCode, importMaps, entrypoint);
  }

  const error = new Error(
    `Worker: Dependency ${id} (resolved: ${moduleId}) not found. Context: ${context}. ` +
      `ImportMaps count: ${Object.keys(importMaps).length}. ` +
      `CWD: ${nodePolyFills.process.cwd()}. ` +
      `Sample ImportMaps keys: ${Object.keys(importMaps).slice(0, 5).join(", ")}`,
  );
  console.error(error);
  throw error;
};

export async function cjs(
  entrypoint: string,
  importMaps: Record<string, string>,
) {
  [pkgJsonCache, resolutionCache, searchPathsCache].forEach((c) => {
    for (const key in c) delete c[key];
  });

  await Promise.all(
    Object.entries(importMaps).map(async ([k, v]) => {
      if (v.startsWith("https://")) {
        try {
          const response = await fetch(v);
          if (response.ok) {
            importMaps[k] = await response.text();
          } else {
            console.error(`Failed to fetch loader '${k}': ${response.status}`);
            delete importMaps[k];
          }
        } catch (error) {
          console.error(`Error fetching loader '${k}':`, error);
          delete importMaps[k];
        }
      }
    }),
  );

  // Suppress SystemJS W3 warnings during module loading
  const originalWarn = console.warn;
  console.warn = (...args: any[]) => {
    const msg = args[0]?.toString() || "";
    if (!msg.includes("(SystemJS Error#W3")) {
      originalWarn.apply(console, args);
    }
  };

  // @ts-ignore
  self.__systemjs_require__ = (id: string) =>
    loadModule(id, path.dirname(entrypoint), importMaps, entrypoint);
  // @ts-ignore
  self.__systemjs_require__.resolve = (request: string) => request;

  // @ts-ignore
  self.createRequire = (filename: string) => {
    const context = path.dirname(filename);
    const req: any = (id: string) =>
      loadModule(id, context, importMaps, entrypoint);
    req.resolve = (request: string) => request;
    return req;
  };

  loadModule(entrypoint, path.dirname(entrypoint), importMaps, entrypoint);
}
