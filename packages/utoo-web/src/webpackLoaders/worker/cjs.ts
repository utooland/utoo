import "systemjs/dist/system.js";

import nodePolyFills from "./nodePolyFills";

export async function cjs(entrypoint: string, importMaps: Record<string, string>,) {
  debugger
  await Promise.all(Object.entries(importMaps).map(async ([k, v]) => {
    if (v.startsWith("http")) {
      try {
        const response = await fetch(v);
        if (response.ok) {
          importMaps[k] = await response.text();
        } else {
          console.error(`Failed to fetch loader '${k}' from ${v}: ${response.status} ${response.statusText}`);
          delete importMaps[k];
        }
      } catch (error) {
        console.error(`Error fetching loader '${k}' from ${v}:`, error);
        delete importMaps[k];
      }
    }
  }));

  Object.assign(importMaps, nodePolyFills);
  const require = (id: string) => {
    let dependency = System.get(id);
    if (dependency) {
      return dependency.default;
    }

    if (id in nodePolyFills) {
      // @ts-ignore
      return nodePolyFills[id];
    }

    const moduleCode = importMaps[id];
    if (!moduleCode) {
      console.error(`Worker: Dependency ${id} not found in import maps.`);
      return {};
    }

    let finalExports = {};
    const module = { exports: finalExports };
    const exports = module.exports;

    try {
      new Function('require', 'exports', 'module', moduleCode)(require, exports, module);
      finalExports = module.exports;

    } catch (e: any) {
      console.error(`Worker: Error executing dependency module ${id}:`, e);
      throw new Error(`Failed to load CJS dependency ${id}: ${e.message}`);
    }

    System.set(id, { default: finalExports });

    return finalExports;
  };

  require.resolve = (request: string) => request

  let entryPointCode: string | undefined;

  for (const path in importMaps) {
    const moduleCode = importMaps[path];

    if (path === entrypoint) {
      entryPointCode = moduleCode;
      // FIXME
      entryPointCode = "self.Buffer = require('buffer').Buffer;" + moduleCode;
      break;
    }


  }

  if (entryPointCode) {
    let finalExports = {};
    const module = { exports: finalExports };
    const exports = module.exports;

    try {
      new Function("require", "exports", "module", entryPointCode)(
        require,
        exports,
        module,
      );
    } catch (e) {
      console.error(`Worker: Error executing entry point ${entrypoint}:`, e);
    }
  } else {
    console.warn(
      "Warning: Entry point not found in import maps. No final execution step taken.",
    );
  }
}

