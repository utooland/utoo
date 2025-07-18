const CHUNK_PUBLIC_PATH = "webpack-loaders.js";
const runtime = require("./[turbopack]_runtime.js");
runtime.loadChunk("[turbopack-node]_transforms_webpack-loaders_ts_a9a573ac.js");
runtime.loadChunk("__8b1952d5.js");
runtime.getOrInstantiateRuntimeModule("[turbopack-node]/globals.ts [webpack_loaders] (ecmascript)", CHUNK_PUBLIC_PATH);
runtime.getOrInstantiateRuntimeModule("[turbopack-node]/ipc/evaluate.ts/evaluate.js { INNER => \"[turbopack-node]/transforms/webpack-loaders.ts [webpack_loaders] (ecmascript)\", RUNTIME => \"[turbopack-node]/ipc/evaluate.ts [webpack_loaders] (ecmascript)\" } [webpack_loaders] (ecmascript)", CHUNK_PUBLIC_PATH);
module.exports = runtime.getOrInstantiateRuntimeModule("[turbopack-node]/ipc/evaluate.ts/evaluate.js { INNER => \"[turbopack-node]/transforms/webpack-loaders.ts [webpack_loaders] (ecmascript)\", RUNTIME => \"[turbopack-node]/ipc/evaluate.ts [webpack_loaders] (ecmascript)\" } [webpack_loaders] (ecmascript)", CHUNK_PUBLIC_PATH).exports;
