/**
 * ESM 构建后为 esm/utils/common.js 注入 __dirname（基于 import.meta.url），
 * 以便 getPackPath() 在 ESM 下能正确解析 pack 根目录。
 * 必须在 add-esm-extensions 之后执行。
 */
const fs = require("fs");
const path = require("path");

const commonPath = path.join(__dirname, "../esm/utils/common.js");
let content = fs.readFileSync(commonPath, "utf8");

const dirnameInject = [
  'import { fileURLToPath } from "node:url";',
  "const __dirname = path.dirname(fileURLToPath(import.meta.url));",
].join("\n");

// 在首个 import 之后插入（紧跟 "import path from ..." 那一行之后）
const firstImportEnd = content.indexOf("\n", content.indexOf('import path from'));
if (firstImportEnd === -1) {
  throw new Error("inject-esm-dirname: could not find first import in common.js");
}
content =
  content.slice(0, firstImportEnd + 1) +
  dirnameInject +
  "\n" +
  content.slice(firstImportEnd + 1);

fs.writeFileSync(commonPath, content, "utf8");
