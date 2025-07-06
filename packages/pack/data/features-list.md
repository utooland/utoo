### Feature Status Legend

* ✅: Completed
* 🟠: Work in Progress
* ❓: To be determined

## Features Status List

| Feature Level1 | Feature Level2 | Feature Status | Feature Details | Remarks |
| :-------------- | :-------------- | :-------------- | :-------------- | :-------------- |
| Mode | `mode` | ✅ | [Webpack `mode` configuration](https://webpack.js.org/configuration/mode/#root) |  |
| Entry | `name` & `import` | ✅ | [Webpack `entry` context](https://webpack.js.org/configuration/entry-context/#entry) |  |
|  | `filename` template | ✅ | [Webpack `output.filename`](https://webpack.js.org/configuration/output/#outputfilename) | e.g., `[name].[contenthash:8].js` |
|  | `library` | ✅ | [Webpack `output.library`](https://webpack.js.org/configuration/output/#outputlibrary) | Supports UMD (root, export) and `dynamicImportToRequire` |
| Module | `rules` | ✅ | [Webpack `module.rules`](https://webpack.js.org/configuration/module/#rulerules) | `loader-runner` supports most mainstream webpack loaders |
| Resolve | `alias` | ✅ | [Webpack `resolve.alias`](https://webpack.js.org/configuration/resolve/#resolvealias) |  |
|  | `extensions` | ✅ | [Webpack `resolve.extensions`](https://webpack.js.org/configuration/resolve/#resolveextensions) |  |
| Define |  | ✅ | [Webpack `DefinePlugin`](https://webpack.js.org/plugins/define-plugin/) |  |
| Externals |  | 🟠 | [Webpack `externals` configuration](https://webpack.js.org/configuration/externals/#root) |  |
| Providers |  | ✅ | [Webpack `ProvidePlugin`](https://webpack.js.org/plugins/provide-plugin/#root) |  |
| Output | `path` | ✅ | [Webpack `output.path`](https://webpack.js.org/configuration/output/#outputpath) |  |
|  | `publicPath` | 🟠 | [Webpack `output.publicPath`](https://webpack.js.org/configuration/output/#outputpublicpath) |  |
|  | `clean` | ✅ | [Webpack `output.clean`](https://webpack.js.org/configuration/output/#outputclean) |  |
|  | `standalone` | ❓ |  |  |
| Target | `browserslist` | ✅ | [Webpack `target` string](https://webpack.js.org/configuration/target/#string) |  |
|  | `node` | 🟠 |  |  |
| Sourcemap |  | ✅ | [Webpack `devtool` configuration](https://webpack.js.org/configuration/devtool/) |  |
| Optimization | `moduleIds` | ✅ | [Webpack `optimization.moduleIds`](https://webpack.js.org/configuration/optimization/#optimizationmoduleids) | Supports "names" or "deterministic" |
|  | `minify` | ✅ | [Webpack `optimization.minimize`](https://webpack.js.org/configuration/optimization/#optimizationminimize) |  |
|  | `splitChunks` | 🟠 | [Turbopack chunking config](https://github.com/vercel/next.js/blob/c3429682aa910eb2b5ddd1e761c8ec8cfaa4bb04/turbopack/crates/turbopack-core/src/chunk/chunking_context.rs#L114) |  |
|  | `treeShaking` | ✅ | [Webpack `tree-shaking` guide](https://webpack.js.org/guides/tree-shaking/#root) | Includes `packageImports` |
|  | `modularizeImports` | ✅ | [UmiJS `babel-plugin-import`](https://github.com/umijs/babel-plugin-import) |  |
| Styles | `less` | ✅ | [Webpack `less-loader`](https://github.com/webpack-contrib/less-loader) |  |
|  | `sass` | ✅ | [Webpack `sass-loader`](https://github.com/webpack-contrib/sass-loader) |  |
|  | `inlineCss` | ✅ | [Webpack `style-loader`](https://github.com/webpack-contrib/style-loader) |  |
|  | `styledJsx` | ✅ | [Vercel `styled-jsx`](https://github.com/vercel/styled-jsx) |  |
|  | `styledComponents` | ✅ | [Styled Components](https://github.com/styled-components/styled-components) |  |
|  | `emotion` | ✅ | [Emotion.js](https://github.com/emotion-js/emotion) |  |
|  | `css parse, transform, minify` | ✅ |  |  |
|  | `css module` | ✅ |  |  |
| Images | `inline` | ✅ | [Webpack `url-loader`](https://github.com/webpack-contrib/url-loader) |  |
|  | `blur placeholder` | ✅ | [Next.js `Image` component](https://nextjs.org/docs/app/api-reference/components/image#blurdataurl) |  |
| MDX |  | ✅ | [MDX.js](https://www.mdxjs.cn/) |  |
| Stats |  | 🟠 | [Webpack `stats` configuration](https://webpack.js.org/configuration/stats/#root) |  |
| Analysis |  | 🟠 | [Webpack Bundle Analyzer](https://github.com/webpack-contrib/webpack-bundle-analyzer) |  |
| Magic Comments | `webpackChunkName` | 🟠 | [Webpack `module` methods](https://webpack.js.org/api/module-methods/#magic-comments) |  |
|  | `webpackIgnore` | 🟠 | [Webpack `module` methods](https://webpack.js.org/api/module-methods/#magic-comments) |  |
| SWC Transform Plugin |  | ✅ | [SWC ECMAScript Plugins](https://swc.rs/docs/plugin/ecmascript/getting-started) |  |
| Module Federation |  | ❓ |  |  |
| CSR |  | 🟠 |  |  |
| SSR |  | ❓ |  |  |
| RSC |  | ❓ |  |  |
| PPR |  | ❓ |  |  |
| Server Action |  | ❓ |  |  |
| Edge Runtime |  | ❓ |  |  |
| HMR |  | 🟠 |  |  |
| Dev Server |  | 🟠 |  |  |
| Lazy Compiling |  | 🟠 |  |  |
| Persistent Caching |  | 🟠 |  |  |
| Bundler Tracing Log | `log file` | ✅ |  |  |
|  | `log viewer` | 🟠 |  |  |

