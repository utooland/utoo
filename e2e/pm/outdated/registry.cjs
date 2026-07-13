const http = require("node:http");

const packages = {
  "@outdated/scope": ["3.0.0", "3.1.0", "4.0.0"],
  "outdated-alias-target": ["5.0.0", "5.1.0", "6.0.0"],
  "outdated-normal": ["1.0.0", "1.2.0", "2.0.0"],
  "outdated-overridden": ["7.0.0", "7.1.0", "8.0.0"],
};

const port = Number(process.argv[2]);
http
  .createServer((request, response) => {
    const pathname = request.url.split("?", 1)[0];
    let name;
    try {
      name = decodeURIComponent(pathname.slice(1));
    } catch {
      response.writeHead(400).end("bad package name");
      return;
    }
    const versions = packages[name];
    if (!versions) {
      response.writeHead(404).end("package not found");
      return;
    }

    const manifest = {
      name,
      "dist-tags": { latest: versions.at(-1) },
      versions: Object.fromEntries(
        versions.map((version) => [version, { name, version }]),
      ),
    };
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify(manifest));
  })
  .listen(port, "127.0.0.1");
