/**
 * After ESM build, inject __dirname (from import.meta.url) into every .js file
 * under esm/ that references __dirname, so ESM output works in Node where __dirname is undefined.
 * Must run after add-esm-extensions.
 *
 * TODO: this is a temporary workaround. Once utoo/pack can act as a mature
 * library bundler with built-in support for __dirname/import.meta.url handling,
 * replace this script with the built-in mechanism instead of post-processing ESM output.
 */
const fs = require("fs");
const path = require("path");

const esmDir = path.join(__dirname, "../esm");

function walk(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) {
      walk(full);
    } else if (e.name.endsWith(".js")) {
      // binding.js must stay as CommonJS-only and should not be post-processed.
      if (e.name === "binding.js") continue;
      injectDirnameIfNeeded(full);
    }
  }
}

function injectDirnameIfNeeded(filePath) {
  let content = fs.readFileSync(filePath, "utf8");
  if (!content.includes("__dirname")) return;

  // Already has our __dirname definition
  if (/const\s+__dirname\s*=\s*path\.dirname\(fileURLToPath\(import\.meta\.url\)\)/.test(content))
    return;

  const lines = content.split("\n");
  let lastImportIndex = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*import\s/.test(lines[i])) lastImportIndex = i;
  }

  const needsPath = !/from\s+["'](?:node:)?path["']/.test(content);
  const needsFileURLToPath = !content.includes("fileURLToPath");

  const injectLines = [];
  if (needsFileURLToPath) injectLines.push('import { fileURLToPath } from "node:url";');
  if (needsPath) injectLines.push('import path from "path";');
  injectLines.push("const __dirname = path.dirname(fileURLToPath(import.meta.url));");

  const insertAt = lastImportIndex >= 0 ? lastImportIndex + 1 : 0;
  const before = lines.slice(0, insertAt).join("\n");
  const after = lines.slice(insertAt).join("\n");
  content = before + "\n" + injectLines.join("\n") + "\n" + after;

  fs.writeFileSync(filePath, content, "utf8");
}

if (!fs.existsSync(esmDir)) {
  console.error("inject-esm-dirname: esm dir not found:", esmDir);
  process.exit(1);
}
walk(esmDir);
