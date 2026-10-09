# Pack Feature Status

This list tracks the implementation progress of `@utoo/pack` on the `next` branch. Feature statuses follow Utoopack's own development roadmap. See the [configuration schema](../config_schema.json), [shared configuration types](../../pack-shared/src/config.ts) and [official configuration guide](https://utoo.land/en/docs/utoopack/config) for option shapes and examples.

Generated from [features-list.json](./features-list.json). After editing the JSON, run `npm run generate-features-list --workspace @utoo/pack`.

### Feature Status Legend

* ✅: Completed
* 🟠: Work in Progress
* ❓: To be determined

## Features Status List

| Feature Level1 | Feature Level2 | Feature Status | Feature Details | Remarks |
| :-------------- | :-------------- | :-------------- | :-------------- | :-------------- |
| Entry | `name` & `import` | ✅ | [Webpack `entry` context](https://webpack.js.org/configuration/entry-context/#entry) | Multiple named entry points |
|  | `library` | ✅ | [Webpack `output.library`](https://webpack.js.org/configuration/output/#outputlibrary) | Library bundles with `name` and `export`; CommonJS and browser globals |
|  | `html` | ✅ | [HTML configuration](../../pack-shared/src/config.ts) | Per-entry templates, inline template content, asset injection and HTML filenames |
|  | HTML entry | ✅ | [HTML entry processing](../src/utils/htmlEntry.ts) | Extract local module scripts from an `.html` entry and use it as the generated HTML template |
| Mode | `mode` | ✅ | [Webpack `mode` configuration](https://webpack.js.org/configuration/mode/#root) |  |
| Module | `rules` | ✅ | [Webpack `module.rules`](https://webpack.js.org/configuration/module/#rulerules) | Glob-keyed conditional loader rules and explicit module types; loader options must be JSON-serializable |
| TypeScript |  | ✅ | [TypeScript snapshots](../../../crates/pack-tests/tests/snapshot/typescript) | Built-in TypeScript transpilation and tsconfig support; type checking runs separately |
| React | `react` | ✅ | [Pack configuration](../config_schema.json) | Automatic or classic JSX runtime and custom `importSource` |
|  | `reactCompiler` | ✅ | [Pack configuration](../config_schema.json) | Opt-in Rust React Compiler; `infer`, `annotation` or `all` modes and React 17/18/19 targets (default 19). React 17/18 require `react-compiler-runtime` |
| Resolve | `alias` | ✅ | [Webpack `resolve.alias`](https://webpack.js.org/configuration/resolve/#resolvealias) | Wildcard aliases and `browser` / `default` conditional aliases |
|  | `extensions` | ✅ | [Webpack `resolve.extensions`](https://webpack.js.org/configuration/resolve/#resolveextensions) |  |
| Externals |  | ✅ | [Webpack `externals` configuration](https://webpack.js.org/configuration/externals/#root) | Object-map configuration for `commonjs`, `esm`, `script`, `global` and `promise` externals |
| Output | `path` | ✅ | [Webpack `output.path`](https://webpack.js.org/configuration/output/#outputpath) |  |
|  | `publicPath` | ✅ | [Webpack `output.publicPath`](https://webpack.js.org/configuration/output/#outputpublicpath) | Static URL prefixes, `runtime` and `auto` |
|  | `crossOriginLoading` | ✅ | [Webpack `output.crossOriginLoading`](https://webpack.js.org/configuration/output/#outputcrossoriginloading) |  |
|  | `clean` | ✅ | [Webpack `output.clean`](https://webpack.js.org/configuration/output/#outputclean) |  |
|  | `filename` | ✅ | [Webpack `output.filename`](https://webpack.js.org/configuration/output/#outputfilename) | Templates override the default short content-hashed names for production client chunks |
|  | `chunkFilename` | ✅ | [Webpack `output.chunkFilename`](https://webpack.js.org/configuration/output/#outputchunkfilename) |  |
|  | `cssFilename` | ✅ | [Webpack `output.cssFilename`](https://webpack.js.org/configuration/output/#outputcssfilename) |  |
|  | `cssChunkFilename` | ✅ | [Webpack `output.cssChunkFilename`](https://webpack.js.org/configuration/output/#outputcsschunkfilename) |  |
|  | `assetModuleFilename` | ✅ | [Webpack `output.assetModuleFilename`](https://webpack.js.org/configuration/output/#outputassetmodulefilename) |  |
|  | `copy` | ✅ | [Mako `config.copy`](https://makojs.dev/docs/config#copy) |  |
|  | `type: "standalone"` | ❓ |  | Schema option only; standalone deployment packaging is not implemented |
|  | `chunkLoadingGlobal` | ✅ | [Pack configuration](../config_schema.json) | Custom chunk registration global for isolating multiple runtimes |
|  | `entryRootExport` | ✅ | [Pack configuration](../config_schema.json) | Expose entry exports on a named `globalThis` property |
| Target | `browserslist` | ✅ | [Webpack `target` string](https://webpack.js.org/configuration/target/#string) | Browserslist query strings via `target`; omit for the default browser build |
|  | `node` | ✅ | [Webpack `target` string](https://webpack.js.org/configuration/target/#string) | `node` and Node.js Browserslist targets such as `node 18` |
| Sourcemap | `sourceMaps` | ✅ | [Webpack `devtool` configuration](https://webpack.js.org/configuration/devtool/) | Boolean source-map control |
| Define |  | ✅ | [Webpack `DefinePlugin`](https://webpack.js.org/plugins/define-plugin/) |  |
| Providers | `provider` | ✅ | [Webpack `ProvidePlugin`](https://webpack.js.org/plugins/provide-plugin/#root) | Module imports or `[module, export]` pairs for free variables |
| Optimization | `concatenateModules` | ✅ | [Webpack `optimization.concatenateModules`](https://webpack.js.org/configuration/optimization/#optimizationconcatenatemodules) | Module concatenation in production builds |
|  | `moduleIds` | ✅ | [Webpack `optimization.moduleIds`](https://webpack.js.org/configuration/optimization/#optimizationmoduleids) | `named` or `deterministic` in production; development always uses `named` |
|  | `minify` | ✅ | [Webpack `optimization.minimize`](https://webpack.js.org/configuration/optimization/#optimizationminimize) |  |
|  | `treeShaking` | ✅ | [Webpack `tree-shaking` guide](https://webpack.js.org/guides/tree-shaking/#root) | ESM and CommonJS tree shaking |
|  | `splitChunks` | 🟠 | [Pack configuration](../config_schema.json) | Turbopack heuristics for production browser builds: JS size/count thresholds and CSS `maxMergeChunkSize` |
|  | `modularizeImports` | ✅ | [UmiJS `babel-plugin-import`](https://github.com/umijs/babel-plugin-import) |  |
|  | `packageImports` | ✅ | [Next.js `optimizePackageImports`](https://nextjs.org/docs/app/api-reference/config/next-config-js/optimizePackageImports) | Configured packages are merged with the built-in optimization list |
|  | `transpilePackages` | ✅ | [Next.js `transpilePackages`](https://nextjs.org/docs/app/api-reference/config/next-config-js/transpilePackages) |  |
|  | `removeConsole` | ✅ | [terser `drop_console`](https://github.com/terser/terser/blob/da1e6fb2acd90e62bac69967718b89d6f00aab79/README.md?plain=1#L734) |  |
|  | `cssChunking` | ✅ | [Pack configuration](../config_schema.json) | Production CSS grouping with `true`, `loose` or `graph`; graph supports `requestCost` and `weightDistribution`. `false` and `strict` are unsupported |
|  | `inferModuleSideEffects` | ✅ | [Pack configuration](../config_schema.json) | Infer side-effect-free modules when package metadata is absent; enabled by default |
|  | `removeUnusedExports` & `removeUnusedImports` | ✅ | [Pack configuration](../config_schema.json) | Remove unused exports and imports across modules; enabled by default in production |
|  | `compress` & `noMangling` | ✅ | [Pack configuration](../config_schema.json) | Compression options and control over local/export name mangling |
|  | `extractComments` | ✅ | [Pack configuration](../config_schema.json) | Extract legal comments to LICENSE files when minifying library output |
|  | `sharedRuntime` | ✅ | [Pack configuration](../config_schema.json) | Share a browser runtime across production application entries; disabled by default and ignored in development, Node.js and library builds |
|  | `nestedAsyncChunking` | ✅ | [Pack configuration](../config_schema.json) | Nested async chunk availability tracking |
| Styles | `less` | ✅ | [Webpack `less-loader`](https://github.com/webpack-contrib/less-loader) | Supports custom `styles.less.loader` and Less implementation/options |
|  | `sass` | ✅ | [Webpack `sass-loader`](https://github.com/webpack-contrib/sass-loader) | Supports custom Sass implementation/options |
|  | `postcss` | ✅ | [PostCss](https://postcss.org/) | Discovered PostCSS config and inline `styles.postcss` plugins run in one pass, with inline plugins appended |
|  | `inlineCss` | ✅ | [Webpack `style-loader`](https://github.com/webpack-contrib/style-loader) | Inject styles at runtime with configurable insertion and injection type |
|  | `styledJsx` | ✅ | [Vercel `styled-jsx`](https://github.com/vercel/styled-jsx) | Built-in styled-jsx transform |
|  | `styledComponents` | ✅ | [Styled Components](https://github.com/styled-components/styled-components) |  |
|  | `css parse, transform, minify` | ✅ | [With lightningcss](https://lightningcss.dev/) |  |
|  | `cssModules` & `autoCssModules` | ✅ |  | CSS Modules, automatic module detection and custom `styles.cssModules.localIdentName`; hash format/length modifiers normalize to `[hash]` |
|  | `emotion` | ✅ | [Pack configuration](../config_schema.json) | Emotion transform with source maps, labels and import maps |
|  | `postcss.implementation` | ✅ | [Pack configuration](../config_schema.json) | Select a custom PostCSS module by absolute path; omitted or null uses runtime resolution |
| Images | `inline` | ✅ | [Webpack `url-loader`](https://github.com/webpack-contrib/url-loader) | Opt in with `images`; `images.inlineLimit` defaults to 10,000 bytes |
|  | `blur placeholder` | ✅ | [Next.js `Image` component](https://nextjs.org/docs/app/api-reference/components/image#blurdataurl) | With `images` enabled, non-inlined PNG/JPEG/WebP/AVIF images export dimensions and blur placeholder metadata |
| Assets | Web Workers | ✅ | [Worker snapshot](../../../crates/pack-tests/tests/snapshot/static-assets/worker_url_dev) | Bundle `new Worker(new URL(..., import.meta.url))` references |
|  | WASM | ✅ | [WASM snapshot](../../../crates/pack-tests/tests/snapshot/static-assets/wasm) | Emit WASM files as static assets with `optimization.wasmAsAsset` |
| MDX | `mdx` | ✅ | [MDX snapshot](../../../crates/pack-tests/tests/snapshot/mdx/basic) | Opt-in Rust MDX transform; boolean or options for JSX runtime, provider and CommonMark/GFM parsing |
| Stats | `stats` | ✅ | [Webpack `stats` configuration](https://webpack.js.org/configuration/stats/#root) | Opt-in Webpack-compatible `stats.json` for client, server and library outputs |
| Analysis | `ANALYZE` | ✅ | [Webpack Bundle Analyzer](https://github.com/webpack-contrib/webpack-bundle-analyzer) | Set `ANALYZE=1` to generate stats and launch the bundle analyzer |
| Magic Comments | `webpackChunkName` | 🟠 | [Webpack `module` methods](https://webpack.js.org/api/module-methods/#magic-comments) | Chunk naming via this comment is not implemented |
|  | `webpackIgnore` | ✅ | [Ignore-comment snapshot](../../../crates/pack-tests/tests/snapshot/ignore_comments) | Supported for `import()`, `require()` and URL references |
| SWC Transform Plugin | `swcPlugins` | ✅ | [SWC ECMAScript Plugins](https://swc.rs/docs/plugin/ecmascript/getting-started) | Custom SWC ECMAScript transform plugins |
| Module Federation |  | ❓ |  | No built-in Module Federation integration |
| HMR |  | ✅ |  | React Fast Refresh, `module.hot`, `import.meta.turbopackHot` and opt-in `devServer.dynamicHmrChunkLists` |
| Dev Server | HTTP / HTTPS / proxy | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Configurable port/host, HTTPS and HTTP proxy rules with path rewriting |
|  | `browserToTerminal` | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Forward browser console output with `devServer.browserToTerminal`: `"error"`, `"warn"` or `true`; disabled by default |
|  | Dependency watching | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Watch selected node_modules packages with `watch.nodeModulesRegexes` or negated `watch.ignored` patterns |
|  | `lazyDynamicImports` | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Compile client `import()` targets on first browser request in development; disabled by default. Pair with HMR and `dynamicHmrChunkLists` |
| Webpack partially compatible mode |  | ✅ | [Webpack compat example](https://github.com/utooland/utoo/tree/next/examples/webpack-compat) | Subset of Webpack configuration, loader and plugin APIs for migration; unsupported options may be ignored or warned about |
| Node Polyfill | `nodePolyfill` | ✅ | [Webpack Node Polyfill Plugin](https://github.com/Richienb/node-polyfill-webpack-plugin) | Opt-in Node.js built-in polyfills for browser builds; disabled by default |
| CSR |  | ✅ |  |  |
| Node.js server bundles | `server.entry` | ✅ | [Multi-server-entry snapshot](../../../crates/pack-tests/tests/snapshot/basic/multi_server_entries) | Single server entry or multiple named entries alongside client entries; the first server entry receives Server Functions |
|  | `server.resolve` | ✅ | [Server resolve snapshot](../../../crates/pack-tests/tests/snapshot/basic/server_alias) | Server aliases override matching shared aliases; server extensions replace the shared list |
|  | `server.externals` | ✅ | [Server externals snapshot](../../../crates/pack-tests/tests/snapshot/externals/server-specific) | Replaces top-level externals when provided; otherwise inherits them |
|  | `server.output` | ✅ | [Pack configuration](../config_schema.json) | Independent output path, entry filename and chunk filename templates |
| SSR |  | ❓ |  | Dedicated SSR rendering and hydration integration is not implemented |
| RSC |  | ❓ |  | React Server Component boundary handling is not implemented |
| Server Functions / RPC | `server.function` | ✅ | [Server references example](../../../examples/with-server-references) | Module-level `"use server"`; application supplies `clientProxy` transport and `serverRegister` modules |
| Edge Runtime |  | ❓ |  | No dedicated Edge Runtime build target |
| Caching | `persistentCaching` | ✅ | [Cache configuration](../src/utils/env.ts) | Enabled by default in development and production; disable with `persistentCaching: false` or `DISABLE_PERSISTENT_CACHE=1` |
|  | `turbopackMemoryEviction` | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Memory eviction modes: `false`, `"auto"` (default) or `"full"`; `true` also selects full eviction |
|  | `cacheDirectory` | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Directory for cache data, locks and traces; defaults to `.turbopack`. Relative paths resolve from the project path |
|  | `turbopackGc` | ✅ | [Shared configuration types](../../pack-shared/src/config.ts) | Opt-in reference-counting GC for unreachable turbo-tasks; disabled by default. Read/write persistent cache mode requires memory eviction |
| Bundler Tracing Log | `log file` | ✅ | [Tracing implementation](../../../crates/pack-napi/src/pack_api/project.rs) | `TURBOPACK_TRACING` writes `.trace-turbopack` in `cacheDirectory` (default `.turbopack`) |
|  | `log viewer` | ✅ | [Turbopack Trace Viewer](https://turbo-trace-viewer.vercel.app/) | Enable `TURBOPACK_TRACING=1` and `TURBOPACK_TRACE_SERVER=1` |
|  | Chrome Trace | ✅ | [Tracing implementation](../../../crates/pack-napi/src/pack_api/project.rs) | Set `TRACING_CHROME=1` or a file path to capture Chrome Trace events |
