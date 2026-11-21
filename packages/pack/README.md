# @utoo/pack

> 🌖 High-performance bundler core for the Utoo toolchain, powered by [Turbopack](https://turbo.build/pack).

`@utoo/pack` is the engine behind the Utoo build system. It leverages the incremental computation power of Turbopack and the performance of Rust to provide a lightning-fast development and build experience.

## ✨ Key Features

- ⚡ **Extreme Performance**: Core bundling logic implemented in Rust via NAPI-RS.
- 🛠️ **Turbopack Powered**: Built on top of the same engine that powers Next.js Turbopack.
- 🔌 **Webpack Compatibility**: Support for consuming `webpack.config.js` to simplify migration from Webpack.
- 📦 **Modern Web Support**: Native support for TypeScript, JSX, CSS Modules, Less, Sass, and more.
- 🔧 **Extensible Architecture**: Support for custom loaders, plugins, and flexible configuration.
- 🔄 **Fast HMR**: Instant updates during development with optimized Hot Module Replacement.

## ✨ Supported Features

`@utoo/pack` aims for high compatibility with the Webpack ecosystem while providing superior performance.

- **Entry**: Supports `name`, `import`, and `filename` templates.
- **Module Rules**: Support for most mainstream Webpack loaders via `loader-runner`.
- **Resolve**: Full support for `alias` and `extensions`.
- **Styles**: Built-in support for Less, Sass, PostCSS, CSS Modules, and LightningCSS.
- **Optimization**: Minification, Tree Shaking, Module Concatenation, and more.
- **Frameworks**: Optimized for React (including `styled-jsx`, `styled-components`).
- **Tools**: Integrated Bundle Analyzer and Tracing Logs.

> [!TIP]
> For a detailed status of all features, see the [Features List](./docs/features-list.md).

## 📦 Installation

```bash
ut install @utoo/pack --save-dev
```

## 🚀 Quick Start

### Programmatic API

You can use `@utoo/pack` directly in your Node.js scripts:

```javascript
const { build, serve } = require('@utoo/pack');

// Production build
async function runBuild() {
  await build({
    config: {
      entry: [
        {
          import: "./src/index.ts",
          html: {
            template: "./index.html"
          }
        }
      ],
      output: {
        path: "./dist",
        filename: "[name].[contenthash:8].js",
        chunkFilename: "[name].[contenthash:8].js",
        clean: true
      },
      sourceMaps: true
    }
  });
}

// Development mode with HMR
async function startDev() {
  const server = await serve({
    config: {
      entry: [
        {
          import: "./src/index.ts",
          html: {
            template: "./index.html"
          }
        }
      ],
      output: {
        path: "./dist",
        filename: "[name].[contenthash:8].js",
        chunkFilename: "[name].[contenthash:8].js",
        clean: true
      },
      sourceMaps: true
    }
  }, undefined, undefined, { port: 3000, returnHandle: true });
  return server;
}
```

`serve()` resolves after the initial build and HTTP startup. By default its result
remains `void`. Pass `{ port: 3000, returnHandle: true }` as the fourth argument
to receive a handle containing
`port`, `hostname`, `writeAllEntrypointsToDisk()` and `close()`. Integrations can
await `writeAllEntrypointsToDisk()` before publishing build results: it checks
compilation diagnostics and regenerates HTML with the current asset paths before
resolving. Compilation and HTML generation errors reject the call. Await
`close()` to release the HTTP server, HMR connections and project resources.
Explicit close stops this Project's input watcher before shutting down its native
task engine, including when persistent caching is disabled. Calling `close()`
does not exit the process. To manage the surrounding process
yourself, pass `{ port: 3000, returnHandle: true, handleSignals: false }` as the fourth argument.

Pass `onClientBuild({ clientPaths, isRebuild })` in the fourth argument to publish
framework HTML after successful client builds. It runs after assets, enabled
stats and built-in HTML are written, even without a connected browser. The first
call has `isRebuild: false` and precedes `onReady`; subsequent calls have
`isRebuild: true`. Its returned promise is awaited before the next output write.
Do not call `writeAllEntrypointsToDisk()` or await `close()` inside this callback.
Startup callback failures reject `serve()`; background failures are logged and
later builds can retry. Failed compilations do not invoke the callback.

### Edit batches from an agent (Node only)

An edit batch groups filesystem watcher changes in the Node dev server. Start it
before writing files, finish **all** asynchronous writes, and do not write again
until the commit result arrives. It is not a filesystem rollback or whole-project
transaction. The hold covers watcher-driven edits by every writer sharing this
Project, including ordinary editor saves. Existing in-flight builds are not cancelled.

Configure the policy only through the fourth `serve()` argument:

```javascript
const server = await serve(options, projectPath, rootPath, {
  port: 3000,
  returnHandle: true,
  editBatch: {
    timeout: { action: "none" },
    afterBuild: "hold",
  },
  onEditBatchEvent(event) {
    // Observation only. The result can always be read again by ID.
    if (event.type === "settled") reportResult(event.result);
  },
});

const batchId = await server.beginEditBatch();
await writeFiles(); // Must drain writes before committing.
const result = await server.commitEditBatch(batchId);
if (result.outcome === "failed") reportErrors(result.errors);
```

Do not commit in a `finally` block: a failed writer may have left a partial edit.
With `none` or `keep-held`, finish or repair the edit before committing the same
batch, or explicitly close the server without releasing its draft.

`commitEditBatch()` resolves after this batch's tracked input invalidations,
fresh strongly consistent entry graph, required output/stats/HTML writes, and
the awaited `onClientBuild` publication callback. It then confirms the
`afterBuild` policy before recording its result. The existing HMR transport is
used; no extra refresh is sent. Browser download/execution/DOM acknowledgement
is not part of this completion condition, and no browser connection is required.

| Policy | Meaning |
| --- | --- |
| `timeout: { action: "none" }` | Keep held until explicit commit or close. |
| `timeout: { action: "keep-held", afterMs }` | Notify once at the deadline, keep the same ID held, and allow a later commit. |
| `timeout: { action: "release", afterMs }` | Release and record `expired` after its ACK. The writer might still be editing, so this is never a controlled `built` result. |
| `afterBuild: "hold"` | Keep the confirmed hold after success or failure; repair requires a new batch and commit. |
| `afterBuild: "resume"` | Confirm watcher release before recording the commit result. |

Omitting `editBatch` defaults to a **30-second release deadline** and
`afterBuild: "resume"`. Explicit-only integrations should choose `none` or
`keep-held`. `afterMs` and wait `timeoutMs` must be positive integers at most
2,147,483,647. The hold deadline starts after native hold acknowledgement and is
cleared when commit wins; a commit and deadline cannot both flush the batch.
`afterBuild` does not re-hold a timeout release.

Results have one of these outcomes:

- `built`: `reason: "commit"`, with confirmed `watchState: "held" | "watching"`.
- `failed`: `reason: "commit" | "timeout" | "close"`, a nonempty `errors` array,
  and the confirmed watch state or `unknown`.
- `expired`: `reason: "timeout"`, `watchState: "watching"`, and
  `error.code: "HOLD_TIMEOUT_RELEASED"`. Subsequent realtime output does not
  change this result into a successful commit.

Failures retain native `NapiIssue` fields (including source positions when
available) and serializable name/message/code/path causes, at most eight levels
per cause chain. Compilation, output I/O, HTML, publication and watcher-control
failures are distinguished. A failed watcher release does not overwrite the
original compilation error. If control fails or is lost and its state cannot be
confirmed, the result is `unknown`; the requested `hold` policy is not evidence
of an actual hold. Close this server before starting another batch.

`waitForEditBatch(batchId, { timeoutMs, signal })` rejoins the existing result
without rebuilding. Its timeout throws `EditBatchWaitTimeoutError` carrying the
ID; abort only ends that waiter. Neither cancels a commit nor releases the hold.
Repeated commits join the same operation or return the same immutable terminal
result. One active batch and the latest 32 terminal results are retained for the
current server lifetime; unknown or evicted IDs return `BATCH_NOT_FOUND`.
Starting another batch while one is active returns `BATCH_BUSY`.

`onEditBatchEvent` receives `hold-timeout` reminders and `settled` results after
storage. Observer throws/rejections are logged and cannot change the result.
By contrast, `onClientBuild` remains an awaited publication step, so its failure
becomes `PUBLISH_CALLBACK_FAILED`. Never await the same batch's commit/wait, a
public all-write, or server close from inside `onClientBuild`: those operations
would wait on their own publication. A `settled` observer may read the stored
result without that self-wait.

Close synchronously prevents a new unrecorded success from winning. After the
existing resources close, an active batch records `failed/SERVER_CLOSED/closed`;
close errors retain their causes and report `unknown`. Pending shutdown is not
reported as closed. An already stored batch result is never rewritten. Closing
an uncommitted batch does not release its draft. The server also disposes its
owned entrypoint/update observers; this adds no global build cancellation API.

For another process, share `UTOO_PACK_AGENT_HMR_TOKEN` with `up dev` (or pass
`agentHmrToken` to `serve()`). The existing HTTP control prefix accepts only
authenticated loopback requests:

```sh
batch_id=$(up hmr begin --port 3000)
# Write and drain all edits.
up hmr commit "$batch_id" --port 3000
# Reconnect after a pending response or a lost connection. This does not build again.
up hmr wait "$batch_id" --port 3000
# If the begin response was lost, omit the ID to read and wait on the active batch.
up hmr wait --port 3000
```

CLI commit/wait print the structured result. Exit codes are 0 for built, 1 for
failed/expired, and 2 for pending. Failed results include their concrete
diagnostics; expired is reported as `HOLD_TIMEOUT_RELEASED`, not a compilation
failure. HTTP commit returns a result instead of the old release-only 204.
A transport wait budget (10 seconds by default, overridable by `timeoutMs` in
HTTP reads/commit requests) returns **202 + batchId** if the operation remains
pending. A disconnected request has no response to send; the server operation
continues. `GET /__utoopack/agent-hmr/active` recovers the active ID, and
`GET /__utoopack/agent-hmr/wait/:id` waits or reads its retained result. No replay
service, persistent idempotency protocol or second policy source is added.
For HTTPS, pass `--https` and ensure Node trusts the certificate.

#### Scope and native regression probes

This capability uses Node NAPI/native watcher control only. `@utoo/web`,
browser-hosted builds, WASM, Workers, Service Workers and their delivery systems
are outside its scope. A browser used in the tests only consumes Node-produced
assets and HMR.

The native flush invalidates existing tracked input read/directory/metadata
sets while held; it does not recursively scan the filesystem, touch the output
filesystem's invalidators, or assume the last OS notification has arrived.
It may cause a full graph recomputation. Delayed events can still produce
ordinary realtime work after resume; exactly-once build counts and zero HTML
writes are not promised.

Watcher hold does **not** isolate direct input-filesystem effect writes or new
request-triggered work. The native input-effect regression probe deliberately
demonstrates that bypass. With `lazyDynamicImports`, an explicit `editBatch`
option rejects startup. A realtime server started without that option remains
available and rejects batch requests before installing hold. Arbitrary plugin
behaviour is not statically detected. Integrations requiring stricter isolation
must validate their fixed plugin/configuration combination; these APIs do not
claim that guarantee for an unverified combination.

Run controller/output/HTTP regressions with the normal Pack tests and CLI
regressions with the Pack CLI tests. After building the local NAPI binary and
Pack CJS output (copying the platform `.node` next to that output), run
`ut run test:native:edit-batch --workspace @utoo/pack`. The committed native
probe covers real file edits, structured syntax errors and recovery, the lazy
configuration boundary, and Node-produced browser HMR/React state. It requires
the repository's Playwright Chromium installation. Run the held filesystem
probes from the `next.js` Cargo workspace with
`cargo test -p turbo-tasks-fs held_`.

### Native Bundle Analyzer

Analyze production client and server applications using Turbopack's module graph,
source-level sizes, treemap and import chains. Switch between Client and Server,
then select an entry to inspect its dependencies:

```bash
up analyze
up analyze --output                         # Export without starting a server
up analyze --output --output-dir reports/bundle
```

The report defaults to `.utoo/analyze`; the interactive command serves it on
`http://127.0.0.1:4000` (`--port` changes the port). Reports contain static assets
and data and can be served by any static HTTP server. Add the report directory
to your project's `.gitignore` if it should not be committed. A subsequent run
replaces a previously generated Pack report; unrelated existing directories are
not overwritten. Keep reports outside the application's client and server build
output directories.

```javascript
const { analyze, serveAnalyzeReport } = require('@utoo/pack');

const report = await analyze({
  config: {
    entry: [
      { name: 'home', import: './src/home.tsx' },
      { name: 'admin', import: './src/admin.tsx' },
    ],
  },
}, process.cwd(), undefined, { outputPath: '.utoo/analyze' });

// Optional: the compiler is already shut down when analyze() resolves.
const server = await serveAnalyzeReport(report.outputPath, 4000);
console.log(server.url);
// await server.close();
```

Analysis uses a separate production configuration with internal source maps and
watching disabled. It writes only the report, without cleaning or emitting the
application's bundles or HTML, and does not generate Webpack stats. Existing
`stats: true` and `ANALYZE=true` behavior in `build()`/`serve()` is unchanged.
Normal builds and development sessions do not collect native analyzer data.

Supported entries include browser applications (HTML and multiple pages), Node
targets, `server.entry` (single or named entries), and Server Functions discovered
from the application. Each entry is labeled `client` or `server` in the returned
`entries` list. Server Functions follow Pack's build semantics: they belong to the
first configured server entry, or the generated `index` entry when none is configured.
Library entries are not supported. Entries refer to Pack entries, not framework
SPA routes; client and server entries are listed separately without inferring
framework route pairings.
Server sizes cover emitted bundles/assets and traced local CommonJS/ESM external
files, including their transitive dependencies and statically discoverable assets.
Like Next.js, the analyzer measures the traced files rather than entire installed
package directories; Node built-ins, remote/global externals, and runtime-provided
externals that cannot be resolved locally have no measured file size. External
files remain external and are shown under **External files** in the
file-type filter (enabled by default), with import chains and per-entry sizes.
The initial/async module indicators describe static import reachability, not
measured browser requests. Compressed module sizes are estimates; their sum is
not an exact HTTP transfer size. External files are measured as stored on disk,
without bundle minification or tree shaking. Analysis itself has additional CPU
and memory cost for source maps, dependency tracing, graph traversal and compression.

The UI and binary data format reuse the pinned Next.js/Turbopack analyzer; see
[`pack-analyzer`](../pack-analyzer/README.md) for source-build details.

#### Export data for scripts and AI

Read an existing report as versioned JSON Lines without loading application
config, compiling, loading the native binding, or starting a server:

```bash
up analyze --output --output-dir reports/bundle-before
up analyze export --report reports/bundle-before > bundle-before.jsonl
up analyze export --report reports/bundle-before --environment server --entry index
```

`--report` defaults to `.utoo/analyze`, relative to the current directory.
`--environment client|server` filters entries; `--entry` accepts an exact name or
an ID such as `/entry-1`. If client and server entries share a name, also select
the environment or use the ID. Exports retain the complete module graph so import
chains remain connected; use each selected entry's exact roots to scope traversal.

Records include metadata, modules with sync/async/traced dependencies, entry
roots, output files with exact membership and sizes, source contributions, and
chunk groups. The public schema documents identities, joins, size attribution,
and coverage:

```bash
node -p "require.resolve('@utoo/pack/analyze/graph-v1.schema.json')"
```

Check `entry.coverage.groups` before treating the group list as complete.
Supported Worker and SharedWorker payloads retain their trigger and output
relationships. Pack does not build service-worker scripts, so registrations
leave chunk-group coverage `unknown`; their absent payload must not be treated
as zero bytes. Known groups are still exported when coverage is `unknown`.
Older reports without an explicit completeness marker also export `unknown`.

The native-free API exposes the same records as an async iterator:

```javascript
const { exportAnalyzeReport } = require('@utoo/pack/analyze');
for await (const record of exportAnalyzeReport('reports/bundle-before', {
  environment: 'server',
})) {
  // Consume each record without collecting the whole export in memory.
  console.log(JSON.stringify(record));
}
```

Check the command's exit status before consuming the result; a malformed later
entry can leave partial JSONL. Errors go to stderr. For pipelines, use Bash's
`set -o pipefail`. Keep before/after captures in separate report directories and
compare the same entries, output classes, and size metric.

For AI audits, rank size contributions, trace the exact imports, and verify
before/after results after a fix. Distinguish server external footprint from
browser downloads and static reachability from measured runtime performance.

## 🔌 Webpack Compatibility Mode

`@utoo/pack` provides a partial compatibility layer for Webpack.

```javascript
const { build } = require('@utoo/pack');
const webpackConfig = require('./webpack.config.js');

async function run() {
  await build({ ...webpackConfig, webpackMode: true });
}
```

> [!NOTE]
> Not all Webpack features and plugins are supported. Check the [Features List](./docs/features-list.md) for details on supported configuration options.

## ⚙️ Configuration

The bundler can be configured via a `utoopack.json` or through the programmatic API. Key configuration areas include:

- **`entry`**: Define your application entry points.
- **`define`**: Build-time variable replacement.
- **`externals`**: Exclude specific dependencies from the bundle.
- **`server.externals`**: Replace top-level externals for server entries and Server Functions. If
  omitted, server builds continue to use top-level `externals`.
- **`devServer.browserToTerminal`**: Forward browser console output to the development terminal.
  Use `"error"`, `"warn"`, `true`, or `false`; standalone Utoopack defaults to `false`.
- **`mode`**: `development` or `production`.

For a full list of options, see the [Configuration Schema](./config_schema.json).

Production client builds default to short content-hashed JS and CSS chunk names,
using Turbopack's 13-character base38 hash (for example, `<hash>.js` or
`turbopack-<hash>.js` for entry runtimes). Development builds retain readable names.
Explicit `output.filename`, `output.chunkFilename`, and `output.cssFilename`
templates take precedence; use `filename: "[name].js"` when consumers require
stable entry filenames. Static assets, copied files, server builds, and library
builds keep their existing naming rules. A deployment's complete URL also includes
its public path and any query parameters, so short chunk names alone do not
guarantee a particular URL length limit.

Set `optimization.sharedRuntime: true` to emit one browser runtime asset shared
across production application entries. This reduces duplicated runtime code in
multi-entry builds and lets pages reuse the cached runtime. Each entry keeps its
own small bootstrap script. The option defaults to `false` because separating the
runtime adds a request for a single entry; it does not apply to development,
Node.js, or library builds.

```json
{
  "mode": "production",
  "optimization": {
    "sharedRuntime": true
  }
}
```

### Worktree and Symlink Debugging

Set `UTOO_ROOT_PATH` to a common parent containing the project and the real paths
of linked dependencies when debugging worktrees or symlinks:

```bash
UTOO_ROOT_PATH=/path/to/common-parent utx up dev
# Relative paths resolve from the current working directory:
UTOO_ROOT_PATH=.. utx up build
```

Precedence is `UTOO_ROOT_PATH` > explicit `rootPath` > automatic detection. Empty
values are ignored. This applies to `build`, `serve`, and direct project creation,
where `rootPath` can be omitted. Updates that omit it retain the existing root.
`findRootDir()` and the project directory stay unchanged, preserving framework
plugin lookup and generated file locations.

## 🛠️ Development

### Prerequisites

- **Rust**: Nightly toolchain (see [rust-toolchain.toml](../../rust-toolchain.toml)).
- **Node.js**: Version 20 or higher.

### Building from Source

```bash
# Build Rust bindings and TypeScript modules
npm run build
```

## 📄 License

[MIT](./LICENSE)
