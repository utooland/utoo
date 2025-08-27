const fs = require("fs");
const path = require("path");

const distPath = path.join(__dirname, "dist");

const assets = fs
  .readdirSync(distPath)
  .filter(
    (p) =>
      fs.statSync(path.join(distPath, p)).isFile() &&
      (p.endsWith(".js") || p.endsWith(".css")),
  );

const html = `<!doctype html>
<html lang="en">
  <body>
    <div id="root"></div>
    ${assets
      .filter((a) => a.endsWith(".js"))
      .map((a) => `<script src="${a}"></script>`)
      .join("\n    ")}
    ${assets
      .filter((a) => a.endsWith(".css"))
      .map((a) => `<link rel="stylesheet" href="${a}">`)
      .join("\n    ")}
  </body>
</html>
`;

fs.writeFileSync(path.join(distPath, "index.html"), html);
