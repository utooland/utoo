# Entry exports

`output.entryRootExport` exposes each application entry's exports through the configured browser
global, `module.exports`, or the configured property on `exports`. Synchronous entries return their
namespace immediately. Entries with top-level await or an asynchronous script external expose a
Promise that resolves to the namespace.

The exporting bootstrap includes its initial JavaScript factories so synchronous consumers do not
wait for chunk downloads. CSS and dynamic imports retain their separate assets. With
`optimization.sharedRuntime`, load the runtime and entry bootstrap in the order returned by the
build endpoint; the runtime is shared while each bootstrap exposes its own entry.

Run the generated-output regression after building the native addon:

```sh
node examples/expose-entry-exports/run-regression.cjs
```

An absolute `.node` path can be supplied as the first argument to test an isolated addon. For a
worktree whose dependency symlink leaves the checkout, set `UTOO_TEST_ROOT` to a directory containing
both the checkout and its dependencies.

The runner builds production and development outputs, parses every emitted JavaScript asset, and
executes the actual bootstraps in browser-like VM realms. It covers multiple initial chunks, live reexports,
CSS, dynamic imports, worker bootstrap ordering, TLA, script externals, shared runtimes, numeric and
named module IDs, minification, and Chrome 41 targeting. The VM runs on current Node; it is not a
historical-browser execution test.
