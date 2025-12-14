import "systemjs/dist/system.js";

import nodePolyFills from "./polyfills/nodePolyFills";

export async function cjs(
  entrypoint: string,
  importMaps: Record<string, string>,
) {
  debugger;
  await Promise.all(
    Object.entries(importMaps).map(async ([k, v]) => {
      if (v.startsWith("http")) {
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

  // Object.assign(importMaps, nodePolyFills);
  const fs = nodePolyFills.fs;
  const path = nodePolyFills.path;

  const existsSync = (p: string) => {
    try {
      fs.statSync(p);
      return true;
    } catch {
      return false;
    }
  };

  const loadModule = (
    id: string,
    context: string = path.dirname(entrypoint),
  ) => {
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

    if (!moduleCode) {
      // Try extensions
      const extensions = ["", ".js", ".json", "/index.js"];
      for (const ext of extensions) {
        const p = resolvedId + ext;
        if (existsSync(p) && !fs.statSync(p).isDirectory()) {
          resolvedId = p;
          moduleCode = fs.readFileSync(p, "utf8") as string;
          moduleId = p;
          break;
        }
      }
    }

    // Fallback: Try resolving relative to parent if not found and looks like a file
    if (!moduleCode && !id.startsWith(".") && !id.startsWith("/")) {
      const relativePath = path.resolve(context, id);
      const extensions = ["", ".js", ".json", "/index.js"];
      for (const ext of extensions) {
        const p = relativePath + ext;
        if (existsSync(p) && !fs.statSync(p).isDirectory()) {
          resolvedId = p;
          moduleCode = fs.readFileSync(p, "utf8") as string;
          moduleId = p;
          break;
        }
      }
    }

    // Fallback: Try resolving absolute paths relative to parent (for generated chunks)
    if (!moduleCode && id.startsWith("/")) {
      const relativePath = path.join(context, id.slice(1));
      const extensions = ["", ".js", ".json", "/index.js"];
      for (const ext of extensions) {
        const p = relativePath + ext;
        if (existsSync(p) && !fs.statSync(p).isDirectory()) {
          resolvedId = p;
          moduleCode = fs.readFileSync(p, "utf8") as string;
          moduleId = p;
          break;
        }
      }
    }

    // Fallback: Try resolving absolute path (handling CWD stripping)
    if (!moduleCode && id.startsWith("/")) {
      // @ts-ignore
      const cwd = self.process?.cwd?.() || self.workerData?.cwd || "/";
      let relativeId = id;
      if (id.startsWith(cwd)) {
        relativeId = id.slice(cwd.length);
        if (relativeId.startsWith("/")) relativeId = relativeId.slice(1);
      }

      const extensions = ["", ".js", ".json", "/index.js"];
      for (const ext of extensions) {
        const p = relativeId + ext;
        if (existsSync(p) && !fs.statSync(p).isDirectory()) {
          resolvedId = p; // Use relative path for FS ops
          moduleCode = fs.readFileSync(p, "utf8") as string;
          moduleId = id; // Keep original absolute path as module ID
          break;
        }
      }
    }

    if (moduleCode) {
      let finalExports = {};
      const moduleRequire = (childId: string) =>
        loadModule(childId, path.dirname(moduleId));
      moduleRequire.resolve = (request: string) => request;

      const module = { exports: finalExports, require: moduleRequire };

      // Hack for entrypoint
      if (moduleId === entrypoint) {
        moduleCode = "self.Buffer = require('buffer').Buffer;" + moduleCode;
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
            moduleCode,
          )(
            moduleRequire,
            module.exports,
            module,
            moduleId,
            path.dirname(moduleId),
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
    }

    console.error(
      `Worker: Dependency ${id} (resolved: ${resolvedId}) not found.`,
    );
    return {};
  };

  // @ts-ignore
  // a hack for loader-runner resolving
  self.__systemjs_require__ = (id: string) =>
    loadModule(id, path.dirname(entrypoint));
  // @ts-ignore
  self.__systemjs_require__.resolve = (request: string) => request;

  loadModule(entrypoint);
}
