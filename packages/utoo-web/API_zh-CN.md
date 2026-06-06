# [`@utoo/web`](https://www.npmjs.com/package/@utoo/web) API 文档

`@utoo/web` 可以在浏览器中运行完整的 Web 开发环境，包括文件系统、依赖管理和构建流程。它集成了 [`utoopack`](https://github.com/utooland/utoo) (Rust + Turbopack)，采用 `wasm32-unknown-unknown` 编译目标，不依赖 Web Container，避免了模拟 Node.js 环境带来的运行时开销（如启动延迟和内存占用）。此外，它利用 Web Workers、Service Workers 和 OPFS 提供流畅的体验。

## 核心概念

1. **Real File System**：项目存在于浏览器的[源私有文件系统（OPFS）](https://developer.mozilla.org/zh-CN/docs/Web/API/File_System_API/Origin_private_file_system)中。`Project` 类提供类似 Node.js `fs` 的接口。
2. **Project Main Worker**：`Project` 实例运行在 Web Worker 中。主线程对象是代理，保持 UI 响应。
3. **Thread Worker**：重度任务（打包、编译）在专用的 Web Worker 中运行，由移植的 `tokio` 运行时驱动。
4. **Loader Worker**：在带有 Node.js polyfills 的专用 Worker 中执行 webpack loaders。
5. **Service Worker**：充当本地服务器，拦截请求并提供构建文件以供预览。

## 文件监听与增量构建

`@utoo/web` 利用现代的 [FileSystemObserver API](https://github.com/whatwg/fs/blob/main/proposals/FileSystemObserver.md) 在浏览器中直接实现高效的文件系统监听。这对于支持 Turbopack 的增量构建能力至关重要。

1.  **FileSystemObserver 集成**：`tokio-fs-ext` crate（由 `utoo-wasm` 使用）提供了一个 `watch` 模块，封装了 `FileSystemObserver` API。这使得 Rust 代码能够接收关于源私有文件系统（OPFS）中文件更改的通知。
2.  **OpfsOffload 层**：[OpfsOffload 的实现](https://github.com/utooland/tokio-fs-ext/tree/master/src/fs/wasm/offload)不仅解决了 JS 对象在 Rust 下不满足线程安全的问题（允许多个 rust 线程并发调用 opfs），也可以以极少的侵入性来扩展 `turbo-tasks-fs` 的文件系统。当检测到文件更改时，事件也会通过此层传播到在 WASM 环境中运行的 Turbopack 引擎。
3.  **增量编译**：Turbopack 的架构建立在响应式图之上。当它接收到文件更改事件时，它仅使依赖图中受影响的部分失效。这触发了仅针对更改的模块及其依赖项的重新计算（重建），从而实现极快的更新。

这种架构确保了 `@utoo/web` 即使对于完全在浏览器中运行的大型项目也能提供响应迅速的开发体验。

---

## 快速上手指南

启动项目主要涉及四个步骤。参考 `examples/utooweb-demo` 或在线体验 [`utoo-repl`](https://utoo-repl.vercel.app)。

### 1. 实例化项目

创建 `Project` 实例，配置 Worker 和 Service Worker。

```typescript
import { Project as UtooProject } from "@utoo/web";
import workerUrl from "@utoo/web/esm/workerInline";
import threadWorkerUrl from "@utoo/web/esm/threadWorkerInline";
import loaderWorkerUrl from "@utoo/web/esm/loaderWorkerInline";

const project = new UtooProject({
    // 文件系统中的项目根目录。
    cwd: "/utooweb-demo",

    // 核心功能内联 Worker data URI。
    workerUrl,

    // 重度任务内联 Worker data URI。
    threadWorkerUrl,

    // webpack loaders 内联 Worker data URI。
    loaderWorkerUrl,

    // WASM 二进制文件 URL。使用 `new URL()` 让打包器自动处理。
    wasmUrl: new URL("@utoo/web/esm/utoo/index_bg.wasm", import.meta.url).href,
    
    // 预览 Service Worker 配置。
    serviceWorker: {
        url: `${location.origin}/serviceWorker.js`,
        scope: "/preview", // Service Worker 控制的路径。
    },
    // 运行 webpack loaders 的 ImportMap
    loadersImportMap: {
      // 接受 umd 脚本 url 或脚本内容字符串
      "xyzLoader": "https://x.y.z.js"
    },

    // 设置 loader import map URL 的 fetch cache mode。
    loadersImportMapFetchCache: "reload"
});
```

Worker 脚本已由 `@utoo/web` 预构建为内联的 base64 data URI 模块。这消除了跨域问题，无需在构建中设置单独的 Worker 入口点。

### 2. 安装 Service Worker

要启用预览功能，您必须注册并安装 Service Worker。

```typescript
await project.installServiceWorker();
```

### 3. 解析和安装依赖

从 `package.json` 解析依赖并安装。也可以将已有的 `package-lock.json` 字符串直接传递给 `install()`。详见 [`project.deps()`](#projectdepsoptions) 和 [`project.install()`](#projectinstallpackagelockjsonstring-maxconcurrentdownloads)。

```typescript
const packageLock = await project.deps();
await project.install(packageLock);
```

### 4. 写入项目文件

将源文件写入文件系统。详见[文件系统方法](#文件系统方法)。

```typescript
import { demoFiles } from "../demoFiles";

await project.mkdir("src");
for (const filePath in demoFiles) {
    await project.writeFile(filePath, demoFiles[filePath]);
}
```

### 5. 构建

直接传递配置对象，或将 `utoopack.json` 写入项目根目录。详见 [`project.build()`](#projectbuildoptions) 和 [`project.dev()`](#projectdevoptions)。

```typescript
await project.build({
  config: {
    entry: [{ import: "./src/index.tsx", name: "main" }],
    output: { path: "dist" },
    stats: true,
  },
});
```

若要使用 loader，请将其与 `loader-runner` 一起添加至 `package.json` 的 `devDependencies` 中并安装。另可通过 [`loadersImportMap`](#new-utooprojectoptions) 配置预构建的 loader 以避免文件系统 I/O 开销。

完成这些步骤后，您的项目就完全初始化并准备好进行交互了。

---

## API 参考

### `new UtooProject(options)`

创建一个新的项目实例。

**选项:**

* `cwd` (string, 必需): 在真实文件系统中作为项目根目录的绝对路径（例如 `/my-app`）。
* `workerUrl` (string, 必需): `Project` 实例核心逻辑实际运行的 Worker 线程的 URL 或内联 data URI。您在主线程中与之交互的 `Project` 对象是一个代理，它将所有核心任务（如文件系统操作）委托给此 Worker。导入预构建的内联版本：`import workerUrl from "@utoo/web/esm/workerInline"`。
* `threadWorkerUrl` (string, 必需): 专用于处理 CPU 密集型任务（如打包和编译）的独立 Worker 线程的 URL 或内联 data URI。导入：`import threadWorkerUrl from "@utoo/web/esm/threadWorkerInline"`。
* `loaderWorkerUrl` (string, 可选): 专用于处理 webpack 加载器的独立 Worker 线程的 URL 或内联 data URI。导入：`import loaderWorkerUrl from "@utoo/web/esm/loaderWorkerInline"`。
* `wasmUrl` (string, 可选): WASM 二进制文件的 URL。使用内联 Worker 时必须显式设置，因为 Blob URL Worker 无法自动解析 WASM 路径。使用 `new URL("@utoo/web/esm/utoo/index_bg.wasm", import.meta.url).href` 让打包器自动处理。
* `serviceWorker` (object, 可选):
  * `url` (string, 必需): Service Worker 脚本的 URL。
  * `scope` (string, 必需): Service Worker 将拦截请求的 URL 范围。这是您预览环境的基路径。
* `loadersImportMap`（对象，可选）：用于配置 webpack Loader 的导入映射。这是一个可选的高级配置。通常情况下，您只需在 `package.json` 中声明 loader 依赖并安装，即可让它工作。配置 `loadersImportMap` 允许您直接提供预构建好的、满足 CommonJS 规范的单一文件（作为 URL 字符串或内容字符串）。这样做可以避免 loader 执行过程中因 `require` 操作产生的文件系统 I/O 开销，从而显著提升构建性能。键是 loader 的名称，值是 UMD/CommonJS 模块的 URL 或内容字符串。loader 将在 Web Worker 池中并行执行。
* `loadersImportMapFetchCache`（RequestCache，可选）：拉取 loader import map URL 时使用的 [`fetch` cache mode](https://developer.mozilla.org/en-US/docs/Web/API/Request/cache)。设置为 `"reload"` 可绕过浏览器缓存。内联脚本内容字符串不受影响。

### 文件系统方法

这些方法是异步的，并模仿了 Node.js `fs` API。

#### `project.writeFile(path, content)`

将内容写入真实文件系统中的文件。如果文件不存在，将会被创建。

* `path` (string): 文件的绝对路径（例如 `/src/index.js`）。
* `content` (string | Buffer): 要写入的内容。

#### `project.readFile(path, encoding)`

读取文件的内容。

* `path` (string): 文件的路径。
* `encoding` (string, 可选): 文件的编码（例如 `'utf8'`）。如果未提供，则返回一个 Buffer。

#### `project.readDir(path)`

读取目录的内容。

* `path` (string): 目录的路径。
* 返回: `Promise<string[]>` - 文件和目录名称的数组。

#### `project.mkdir(path)`

创建一个新目录。

* `path` (string): 要创建的目录的路径。

#### `project.rm(path, options)`

删除一个文件或目录。

* `path` (string): 要删除的文件或目录的路径。
* `options` (object, 可选):
  * `recursive` (boolean): 如果为 `true`，则执行递归目录删除。默认为 `false`。

#### `project.rmdir(path)`

删除一个目录。

* `path` (string): 要删除的目录的路径。

### 依赖管理

已安装的依赖包以逻辑链接的形式指向 OPFS 中的全局共享存储。这意味着同一浏览器域名下的不同项目可以共享同名且同版本的依赖，无需重复下载——机制类似于 `pnpm` 的存储策略。

这种设计具有以下优势：
1. **节省存储空间**：相同版本的依赖包在 OPFS 中仅存储一份。
2. **加速项目初始化**：已存在的依赖包可直接复用，实现秒级安装。
3. **减少网络流量**：常用依赖包只需下载一次。
4. **跨标签页复用**：同一域名下的浏览器标签页可直接共享依赖。

#### `project.deps(options?)`

从项目的 `package.json` 解析依赖并生成 lock 文件字符串。这使得在浏览器中直接进行依赖解析成为可能，无需预先准备 `package-lock.json`。

**选项：**

* `registry` (string, 可选): 用于获取包元数据的 npm registry URL。默认为 `https://registry.npmmirror.com`。你可以使用任何与 npm 兼容的 registry，包括私有 registry。
* `concurrency` (number, 可选): 获取包元数据的最大并发网络请求数。默认为 `20`。

**返回值：** `Promise<string>` - 表示解析后的依赖 lock 文件的 JSON 字符串，与 `package-lock.json` 格式兼容。

**示例：**

```typescript
// 使用默认 registry（npmmirror）
const lockFile = await project.deps();

// 使用官方 npm registry
const lockFile = await project.deps({
    registry: "https://registry.npmjs.org"
});

// 使用私有 registry 并自定义并发数
const lockFile = await project.deps({
    registry: "https://npm.mycompany.com",
    concurrency: 10
});
```

#### `project.install(packageLockJsonString, maxConcurrentDownloads?)`

根据 lock 文件字符串填充 `node_modules` 目录。

* `packageLockJsonString` (string): 依赖 lock 文件的 JSON 字符串（来自 `deps()` 或 `package-lock.json` 文件）。
* `maxConcurrentDownloads` (number, 可选): 最大并发包下载数。

### 预览功能

#### `project.installServiceWorker()`

注册并激活构造函数中定义的 Service Worker。这对于预览功能至关重要。

#### `project.build(options?)`

在线程 Worker 中触发构建过程。

**选项:**

* `config` (`ConfigComplete`, 可选): 打包器配置对象。当提供此参数时，不会从磁盘读取 `utoopack.json` 文件。完整类型定义参见 `@utoo/pack-shared`。
* `cleanup` (boolean, 可选): 当为 `true` 时，会销毁现有的全局项目实例并创建新实例，丢弃所有缓存的 turbo-tasks 状态。默认为 `false`。

* 返回: `Promise<BuildOutput>` - Promise 会在构建完成时解析，返回包含 issues 和 diagnostics 的构建输出。

**示例:**

```typescript
// 使用磁盘上的 utoopack.json 构建
await project.build();

// 使用内联配置构建
await project.build({
  config: {
    entry: [{ import: "./src/index.tsx", name: "main" }],
    output: { path: "dist" },
  },
});

// 强制重新创建项目实例
await project.build({ cleanup: true });
```

### 开发模式与 HMR

#### `project.dev(options?)`

启动开发模式，支持文件监听和热模块替换（HMR）。与 `build()` 不同，`dev()` 会持续监听文件变化并自动触发增量构建。

**选项:**

* `config` (`ConfigComplete`, 可选): 打包器配置对象。当提供此参数时，不会从磁盘读取 `utoopack.json` 文件。
* `onUpdate` (function, 可选): 当构建完成时的回调函数，接收构建结果（包含 issues 和 diagnostics）。

**示例:**

```typescript
// 带回调的开发模式
project.dev({
  onUpdate: (result) => {
    console.log('构建完成', result.issues);
  },
});

// 带内联配置的开发模式
project.dev({
  config: {
    entry: [{ import: "./src/index.tsx", name: "main" }],
    output: { path: "dist" },
  },
  onUpdate: (result) => {
    console.log('构建完成', result.issues);
  },
});
```

#### `project.updateInfoSubscribe(aggregationMs, callback)`

订阅编译生命周期事件。这取代了旧版本中的 `onUpdateStart` 和 `onUpdateEnd` 钩子。

**参数:**

* `aggregationMs` (number): 聚合时间（毫秒）。
* `callback` (function): 接收 `UpdateMessage` 对象的回调函数。

**示例:**

```typescript
project.updateInfoSubscribe(100, (message) => {
  if (message.updateType === "start") {
    console.log('构建开始...');
  } else if (message.updateType === "end") {
    const { duration, tasks } = message.value;
    console.log(`构建完成，耗时 ${duration}ms，任务数 ${tasks}`);
  }
});
```

#### `project.connectHmrIframe(iframe, origin?)`

将预览 iframe 连接到 HMR 服务器。返回一个 `HmrClient` 实例。

* `iframe` (HTMLIFrameElement): 预览页面的 iframe 元素。
* `origin` (string, 可选): 预览页面的源（默认为 `*`）。
* 返回: `HmrClient | null` - HMR 客户端实例，如果尚未调用 `dev()` 则返回 `null`。调用 `client.close()` 断开 HMR 连接。

**示例:**

```typescript
const iframeRef = useRef<HTMLIFrameElement>(null);

useEffect(() => {
  if (!iframeRef.current) return;
  
  const client = project.connectHmrIframe(iframeRef.current);
  
  return () => {
    client?.close();
  };
}, [project]);
```

#### HMR 工作原理

HMR 通过 MessagePort 实现主线程与预览 iframe 之间的通信：

1. 调用 `project.dev()` 启动开发模式，这会启动文件监听和 HMR 服务器
2. 调用 `project.connectHmrIframe(iframe)` 将 iframe 连接到 HMR 服务器
3. 当文件变化时，Turbopack 自动触发增量构建
4. HMR 更新通过 MessagePort 发送到 iframe
5. iframe 中的 HMR 客户端应用更新，无需刷新页面

---

## 示例工作流：构建和预览

`utooweb-demo` 展示了一个完整的编辑、构建和预览的工作流。

1. **编辑**: 使用 `project.readFile()` 读取文件并显示在编辑器中。当内容更改时，调用 `project.writeFile()`（通常带有防抖动）将更改保存回 OPFS。

    ```typescript
    // 在 useFileContent.ts 中
    const content = await project.readFile(filePath, "utf8");
    // ...
    await project.writeFile(selectedFilePath, newContent);
    ```

2. **构建**: 用户点击“构建”按钮，调用 `project.build()`。

    ```typescript
    // 在 useBuild.ts 中
    setIsBuilding(true);
    try {
      await project.build();
        // 构建成功
    } catch (e) {
        // 构建失败
    } finally {
        setIsBuilding(false);
    }
    ```

3. **处理构建输出**: 构建成功后，应用程序读取构建输出（例如 `dist/stats.json`）以查找生成的资源文件（`.js`、`.css`）。然后它会生成一个包含这些资源的 `index.html`。生成 HTML 的逻辑类似于 `html-webpack-plugin`。我们目前正在计划将 HTML 直接作为构建入口（entry），在完成该特性之后，可以省去手动生成 HTML 这一步。

    ```typescript
    // 在 useBuild.ts 中
    const statsContent = await project.readFile("dist/stats.json", "utf8");
    const stats = JSON.parse(statsContent);
    // ... 生成带有正确 script/link 标签的 HTML 的逻辑 ...
    await project.writeFile("dist/index.html", generatedHtml);
    ```

4. **预览**: `Preview` 组件包含一个 `iframe`，其 `src` 指向 Service Worker 范围内的入口点（例如 `/preview/dist/index.html`）。构建完成后，`iframe` 会重新从 Service Worker 加载 OPFS 中新生成的产物文件。

这个循环提供了一个快速、交互式的开发循环，全部在用户的浏览器中本地运行。

---

## 服务器配置：COOP & COEP 头

要使用 `@utoo/web`，您的开发服务器必须提供带有特定 HTTP 头的应用程序，以创建一个**跨源隔离环境**。这是浏览器为启用像 `SharedArrayBuffer` 这样的强大功能而强制执行的安全要求，这些功能对于底层 WebAssembly 组件的多线程性能至关重要。

您必须配置服务器以发送以下两个头：

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

#### 示例: `webpack-dev-server`

如果您正在使用 `webpack-dev-server`，您可以在 `webpack.config.js` 文件中添加这些头，如 `utooweb-demo` 中所示：

```javascript
// webpack.config.js
module.exports = {
  // ... 其他配置
  devServer: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    // ... 其他 devServer 选项
  },
};
```

**重要提示**: 如果没有这些头，浏览器将不会启用必要的功能，`@utoo/web` 将无法初始化。此配置是托管您应用程序的任何服务器所必需的，不仅仅是 `webpack-dev-server`。

---

## 设置 Worker 脚本

`@utoo/web` 提供预构建的内联 Worker，以 base64 data URI 模块的形式发布。只需导入并传递给构造函数即可——无需设置单独的构建入口或处理跨域配置。

```typescript
import workerUrl from "@utoo/web/esm/workerInline";
import threadWorkerUrl from "@utoo/web/esm/threadWorkerInline";
import loaderWorkerUrl from "@utoo/web/esm/loaderWorkerInline";
```

内联 Worker 解决了**跨域 Worker 限制**：当应用从 CDN 或不同域名提供服务时，`new Worker(url)` 会因同源策略而失败。内联 Worker 通过将 Worker 代码嵌入为 data URI 并在运行时创建 Blob URL 来绕过此限制。

#### 服务 Worker (`serviceWorker.ts`)

Service Worker **不能**被内联（浏览器要求 `navigator.serviceWorker.register()` 使用真实 URL）。在项目中创建一个简单的包装文件：

```typescript
// src/serviceWorker.ts
import "@utoo/web/esm/serviceWorker";
```

配置打包器将此文件输出为单独的文件，然后将其 URL 传递给构造函数。

#### `createWorkerFromDataUri(dataUri, options?)`

从 `@utoo/web` 导出的工具函数，用于从 data URI 字符串创建 Worker。内部使用 Blob URL 以确保跨浏览器兼容性（Firefox 不支持直接使用 `new Worker("data:...")` ）。`Project` 类在检测到 `data:` URI 时会自动使用此函数，通常无需手动调用。

## 注意

* 由于当前 Rust 上默认的内存分配器 [`dlmalloc`](https://github.com/alexcrichton/dlmalloc-rs) 在多线程 `wasm` 上性能不够理想，我们将 [`mimalloc`](https://github.com/microsoft/mimalloc) 移植到了 wasm32-unknown-unknown 平台，以支持开启 CPU 核心数量的线程来运行构建。因此在浏览器环境和在操作系统环境，构建的性能差异十分微小。
* turbopack 的部分高级功能如[`持久化缓存`](https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopackPersistentCaching)，目前也在计划之中，未来会在浏览器内直接支持。
