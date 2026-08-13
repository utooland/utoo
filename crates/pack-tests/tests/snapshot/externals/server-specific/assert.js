const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const serverOutput = fs.readFileSync(
  path.join(__dirname, "output", "server", "server.js"),
  "utf8",
);

assert(
  serverOutput.includes(
    '"[externals]/server-only [external] (server-only, cjs)"',
  ),
  "server should use server.externals",
);
assert(
  serverOutput.includes('require("server-only")'),
  "server external should be emitted as a CommonJS require",
);
assert(
  serverOutput.includes("/client-only/index.js [server]"),
  "top-level external should be bundled when server.externals replaces it",
);
assert(
  !serverOutput.includes("[externals]/client-only [external]"),
  "server.externals should replace top-level externals",
);
