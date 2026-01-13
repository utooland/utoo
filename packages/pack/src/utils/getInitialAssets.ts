import fs from "fs";
import { join } from "path";

export interface Assets {
  js: string[];
  css: string[];
}

export function getInitialAssetsFromStats(outputDir: string): Assets {
  const assets = { js: [] as string[], css: [] as string[] };
  const statsPath = join(outputDir, "stats.json");
  if (fs.existsSync(statsPath)) {
    try {
      const stats = JSON.parse(fs.readFileSync(statsPath, "utf-8"));
      if (stats.entrypoints) {
        Object.values(stats.entrypoints).forEach((entrypoint: any) => {
          entrypoint.assets?.forEach((asset: any) => {
            if (asset.name.endsWith(".js") && !assets.js.includes(asset.name)) {
              assets.js.push(asset.name);
            }
            if (
              asset.name.endsWith(".css") &&
              !assets.css.includes(asset.name)
            ) {
              assets.css.push(asset.name);
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
