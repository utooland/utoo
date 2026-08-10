const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

function modules(output) {
  const stats = JSON.parse(
    fs.readFileSync(path.join(__dirname, "output", output, "stats.json")),
  );
  return stats.modules;
}

function hasExternal(modules, name) {
  return modules.some(
    (module) => module.name === name && module.id.includes("[external]"),
  );
}

const clientModules = modules("client");
assert(
  hasExternal(clientModules, "global ClientOnly"),
  "client should use top-level externals",
);
assert(
  !hasExternal(clientModules, "server-only"),
  "server.externals must not affect the client",
);

const serverModules = modules("server");
assert(
  hasExternal(serverModules, "server-only"),
  "server should use server.externals",
);
assert(
  !hasExternal(serverModules, "global ClientOnly"),
  "server.externals should replace top-level externals",
);
