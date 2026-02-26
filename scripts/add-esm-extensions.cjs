"use strict";

const fs = require("fs");
const path = require("path");

// Usage: node add-esm-extensions.cjs [esmDir]
// Run from package root (e.g. packages/pack); default esmDir = "esm"
const ESM_DIR = path.resolve(process.cwd(), process.argv[2] || "esm");

function addJsExtension(content) {
  // Relative import/export: ./path or ../path without extension -> add .js (Node ESM requires it)
  return content.replace(
    /(from\s+['"]|export\s+\*\s+from\s+['"])(\.\.?\/[^'"]+)(['"])/g,
    (_, prefix, specifier, quote) => {
      if (/\.(js|mjs|cjs|json|node)$/i.test(specifier)) return prefix + specifier + quote;
      return prefix + specifier + ".js" + quote;
    }
  );
}

function walk(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) walk(full);
    else if (e.name.endsWith(".js")) {
      const content = fs.readFileSync(full, "utf8");
      fs.writeFileSync(full, addJsExtension(content), "utf8");
    }
  }
}

if (!fs.existsSync(ESM_DIR)) {
  console.error("esm dir not found:", ESM_DIR);
  process.exit(1);
}
walk(ESM_DIR);
