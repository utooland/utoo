const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

function readJavaScriptFiles(directory) {
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const entryPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        return readJavaScriptFiles(entryPath);
      }
      return entry.isFile() && entry.name.endsWith(".js")
        ? fs.readFileSync(entryPath, "utf8")
        : [];
    })
    .join("\n");
}

for (const target of ["client", "server"]) {
  const output = readJavaScriptFiles(path.join(__dirname, "output", target));
  assert.match(
    output,
    /__turbopack_context__\.s\(\[\s*"thisIsAVeryLongExportName"/,
    `${target} export name should be preserved when noMangling is enabled`,
  );
}
