import fs from "fs";
import path from "path";
import type { NapiWrittenEndpoint } from "../binding";

export interface Assets {
  js: string[];
  css: string[];
}

function addUniqueAsset(assets: string[], file: string) {
  if (!assets.includes(file)) {
    assets.push(file);
  }
}

function isJavascriptAsset(file: string): boolean {
  return (
    file.endsWith(".js") &&
    !file.endsWith(".LICENSE.txt") &&
    !file.endsWith(".map")
  );
}

export function getInitialAssetsFromStats(outputDir: string): Assets {
  const assets = { js: [] as string[], css: [] as string[] };
  const statsPath = path.join(outputDir, "stats.json");
  if (fs.existsSync(statsPath)) {
    try {
      const stats = JSON.parse(fs.readFileSync(statsPath, "utf-8"));
      if (stats.entrypoints) {
        Object.values(stats.entrypoints).forEach((entrypoint: any) => {
          entrypoint.assets?.forEach((asset: any) => {
            if (asset.name.endsWith(".js")) {
              addUniqueAsset(assets.js, asset.name);
            }
            if (asset.name.endsWith(".css")) {
              addUniqueAsset(assets.css, asset.name);
            }
          });
        });
      }
    } catch (e) {
      console.warn("Failed to read stats.json for assets discovery", e);
    }
  }
  return assets;
}

export function getInitialAssetsFromEndpointPaths(
  endpoints: NapiWrittenEndpoint[],
): Assets {
  const assets = { js: [] as string[], css: [] as string[] };

  endpoints.forEach((endpoint) => {
    endpoint.clientPaths.forEach((file) => {
      if (isJavascriptAsset(file)) {
        addUniqueAsset(assets.js, file);
      }
      if (file.endsWith(".css")) {
        addUniqueAsset(assets.css, file);
      }
    });
  });

  return assets;
}
