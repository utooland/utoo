# Browser module references

Framework integrations can provide `module.references` to replace exact modules
with generated client references. The framework discovers the exports and decides
which browser-visible data to pass to its runtime. Utoopack does not interpret
decorators, routes, transport protocols, or class semantics.

```ts
import { defineConfig, moduleReferencesVersion } from "@utoo/pack";

if (moduleReferencesVersion !== 1) {
  throw new Error("The loaded native binary does not support module references");
}

export default defineConfig({
  // ...normal build options
  module: {
    references: [{
      resource: "src/greeting.ts",
      references: [{
        id: "greeting",
        factory: {
          module: "./client-runtime.js",
          export: "createReference",
        },
        data: { operation: "greet" },
      }],
      exports: [
        { name: "Greeting", reference: "greeting" },
        { name: "default", reference: "greeting" },
      ],
    }],
  },
});
```

The generated module is equivalent to:

```js
import { createReference } from "./client-runtime.js";
const reference = createReference({ operation: "greet" });
export { reference as Greeting, reference as default };
```

Both exports refer to the same value. The runtime factory chooses whether that
value is a constructor, callable function, or another client reference. Creating
a class instance and sending a request are runtime responsibilities.

`resource` must identify a file within the project, using a project-relative path
or an absolute path. Supply the canonical resolved file path when using aliases
or symlinks. Relative factory imports resolve from the replaced file's directory;
absolute paths and package specifiers follow normal module resolution.

Matching uses the resolved file path, not the basename or export name. Aliases,
symlink imports, and barrel reexports that resolve to the configured file receive
the same replacement; unrelated files with the same name keep their original
behavior. A query string does not bypass replacement, but different queries keep
their normal distinct module identities. Omitting `module.references` or supplying
an empty list leaves normal module processing in place.

The browser build replaces the source before parsing and dependency traversal,
and bypasses user loaders for that source. Its generated identity contains only
reference code, including in source maps. The original implementation remains
unchanged in Node builds. The feature does not build or deploy a second server
application or produce a separate transport manifest.

Reference ids and export names must be unique within a resource; every export
must refer to a declared id. Duplicate resources, missing references, and invalid
configuration fail the build. Only data intended for browser consumption belongs
in `data`: it is serialized directly into the client bundle.

`moduleReferencesVersion` is read from the loaded native binary. The JS wrapper
also rejects nonempty reference configuration when the native binary is older,
including when a newer JS package is linked to an older local native build.

This initial contract applies to ordinary JavaScript module imports. Importing a
registered source explicitly as a text/bytes asset or with an inline loader fails
before reading its implementation. Runtime objects with
constructors or methods must be represented by the factory, not embedded in JSON.
