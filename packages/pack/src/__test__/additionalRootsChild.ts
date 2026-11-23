import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { stripVTControlCharacters } from "node:util";
import type { ConfigComplete } from "../config/types";

const [, , fixturePath] = process.argv;
assert(fixturePath);
const fixture = fs.realpathSync(fixturePath);
const workspace = path.join(fixture, "workspace");
const app = path.join(workspace, "app");
const library = path.join(workspace, "library");
const globalModules = path.join(fixture, "global-prefix", "lib/node_modules");
const packageName = "@link-repro/component-library";
const unlistedPackageName = "unlisted-package";

function linkPackage(name: string, target: string) {
  // Match the two links created by globally registering a package and linking it into the app.
  const globalLink = path.join(globalModules, name);
  const appLink = path.join(app, "node_modules", name);
  fs.mkdirSync(path.dirname(globalLink), { recursive: true });
  fs.mkdirSync(path.dirname(appLink), { recursive: true });
  fs.symlinkSync(target, globalLink, "junction");
  fs.symlinkSync(globalLink, appLink, "junction");
}

fs.mkdirSync(library, { recursive: true });
linkPackage(packageName, library);
fs.writeFileSync(
  path.join(library, "package.json"),
  JSON.stringify({
    name: packageName,
    type: "module",
    exports: { ".": "./main.js", "./ssr": "./ssr.js" },
  }),
);
fs.writeFileSync(
  path.join(app, "package.json"),
  JSON.stringify({ name: "linked-app", private: true }),
);
fs.writeFileSync(
  path.join(app, "index.js"),
  `import { value } from "${packageName}"; console.log(value);`,
);
fs.writeFileSync(
  path.join(library, "main.js"),
  'export const value = "linked-main-marker";',
);
fs.writeFileSync(
  path.join(library, "ssr.js"),
  'export const value = "linked-ssr-marker";',
);

const unlisted = path.join(fixture, "unlisted");
fs.mkdirSync(unlisted);
linkPackage(unlistedPackageName, unlisted);
fs.writeFileSync(
  path.join(unlisted, "package.json"),
  JSON.stringify({ name: unlistedPackageName, type: "module" }),
);
fs.writeFileSync(path.join(unlisted, "index.js"), "export default 1;");
fs.writeFileSync(
  path.join(app, "unlisted.js"),
  `import value from "${unlistedPackageName}/index.js"; console.log(value);`,
);

function assertLinkedOutput(output: string, entry: "main" | "ssr") {
  const dist = path.join(app, output);
  const files = fs.readdirSync(dist, { recursive: true });
  assert.ok(
    files.some(
      (file) =>
        file.endsWith(".js") &&
        fs
          .readFileSync(path.join(dist, file), "utf8")
          .includes(`linked-${entry}-marker`),
    ),
    `The bundle should contain the linked ${entry} module`,
  );
}

async function verifyLinkedEntries() {
  if (process.env.NAPI_RS_NATIVE_LIBRARY_PATH) {
    // Let domparser-rs load its own native addon.
    await import("../binding");
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
  const { build } = await import("../commands/build");
  for (const entry of ["main", "ssr"] as const) {
    const output = `dist-${entry}`;
    const config: ConfigComplete = {
      entry: [{ import: "./index.js" }],
      output: { path: output, clean: true },
      persistentCaching: false,
      resolve: {
        alias: entry === "ssr" ? { [packageName]: `${packageName}/ssr` } : {},
      },
    };
    await assert.rejects(
      build({ config, tracing: false }, app, workspace),
      (error: Error) => {
        const message = stripVTControlCharacters(error.message);
        assert.ok(
          message.includes(`Module not found: Can't resolve '${packageName}'`),
          message,
        );
        if (entry === "ssr") {
          assert.ok(message.includes("with subpath '/ssr'"), message);
        }
        return true;
      },
    );

    const additionalRoots = {
      tnpm: {
        path:
          entry === "main" ? globalModules : path.relative(app, globalModules),
      },
    };
    const options = { config: { ...config, additionalRoots }, tracing: false };
    await build(options, app, workspace);
    assertLinkedOutput(output, entry);

    if (entry === "main") {
      const cacheDirectory = path.join(app, ".cache");
      const cachedOptions = {
        ...options,
        buildId: "additional-roots-cache-test",
        config: { ...options.config, persistentCaching: true, cacheDirectory },
      };
      await build(cachedOptions, app, workspace);
      assertLinkedOutput(output, entry);
      const cacheFiles = fs.readdirSync(cacheDirectory, { recursive: true });
      assert.ok(
        cacheFiles.some((file) => path.basename(file) === "CURRENT") &&
          cacheFiles.some((file) => file.endsWith(".sst")),
        "Expected persisted Turbo Tasks snapshot",
      );

      await build(cachedOptions, app, workspace);
      assertLinkedOutput(output, entry);
    }
  }

  await assert.rejects(
    build(
      {
        config: {
          entry: [{ import: "./unlisted.js" }],
          output: { path: "dist-unlisted" },
          persistentCaching: false,
          additionalRoots: { tnpm: { path: globalModules } },
        },
        tracing: false,
      },
      app,
      workspace,
    ),
    (error: Error) => {
      assert.match(
        stripVTControlCharacters(error.message),
        /Module not found: Can't resolve 'unlisted-package\/index\.js'/,
      );
      return true;
    },
  );
}

verifyLinkedEntries().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
