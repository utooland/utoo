import fs from "fs";
import path from "path";

export interface Assets {
  js: string[];
  css: string[];
}

function addUniqueAsset(assets: string[], file: string) {
  if (!assets.includes(file)) {
    assets.push(file);
  }
}

function entryNameFromImport(entryImport: string): string {
  const basename = path.basename(entryImport);
  const extname = path.extname(basename);
  return extname ? basename.slice(0, -extname.length) : basename;
}

function isJavascriptAsset(file: string): boolean {
  return (
    file.endsWith(".js") &&
    !file.endsWith(".LICENSE.txt") &&
    !file.endsWith(".map")
  );
}

function findEntryJavascript(
  filenames: string[],
  entryName: string,
): string | undefined {
  const exactFile = `${entryName}.js`;
  if (filenames.includes(exactFile)) {
    return exactFile;
  }

  const hashedPrefix = `${entryName}.`;
  return filenames
    .filter((file) => isJavascriptAsset(file) && file.startsWith(hashedPrefix))
    .sort()[0];
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

export function getInitialAssetsFromOutput(
  outputDir: string,
  entryImports: string[],
): Assets {
  const assets = { js: [] as string[], css: [] as string[] };
  let filenames: string[];

  try {
    filenames = fs.readdirSync(outputDir);
  } catch {
    return assets;
  }

  for (const entryImport of entryImports) {
    const entryJs = findEntryJavascript(
      filenames,
      entryNameFromImport(entryImport),
    );
    if (entryJs) {
      addUniqueAsset(assets.js, entryJs);
    }
  }

  if (assets.js.length === 0) {
    const jsFiles = filenames.filter((file) => isJavascriptAsset(file)).sort();
    if (jsFiles.length === 1) {
      addUniqueAsset(assets.js, jsFiles[0]);
    }
  }

  filenames
    .filter((file) => file.endsWith(".css"))
    .sort()
    .forEach((file) => addUniqueAsset(assets.css, file));

  return assets;
}
