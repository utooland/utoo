# [`@utoo/web`](https://www.npmjs.com/package/@utoo/web) API 文档

`@utoo/web` 可以在浏览器中运行完整的 Web 开发环境，包括文件系统、依赖管理和构建流程。它集成了 [`utoopack`](https://github.com/utooland/utoo) (Rust + Turbopack)，并利用 Web Workers、Service Workers 和 OPFS 提供流畅的体验。

## 核心概念

1. **Real File System**：项目存在于浏览器的[源私有文件系统（OPFS）](https://developer.mozilla.org/zh-CN/docs/Web/API/File_System_API/Origin_private_file_system)中。`Project` 类提供类似 Node.js `fs` 的接口。
2. **Project Main Worker**：`Project` 实例运行在 Web Worker 中。主线程对象是代理，保持 UI 响应。
3. **Thread Worker**：重度任务（打包、编译）在专用的 Web Worker 中运行，由移植的 `tokio` 运行时驱动。
4. **Loader Worker**：在带有 Node.js polyfills 的专用 Worker 中执行 Webpack loaders。
5. **Service Worker**：充当本地服务器，拦截请求并提供构建文件以供预览。

## 文件监听与增量构建

`@utoo/web` 利用现代的 [FileSystemObserver API](https://github.com/whatwg/fs/blob/main/proposals/FileSystemObserver.md) 在浏览器中直接实现高效的文件系统监听。这对于支持 Turbopack 的增量构建能力至关重要。

1.  **FileSystemObserver 集成**：`tokio-fs-ext` crate（由 `utoo-wasm` 使用）提供了一个 `watch` 模块，封装了 `FileSystemObserver` API。这使得 Rust 代码能够接收关于源私有文件系统（OPFS）中文件更改的通知。
2.  **OpfsOffload 层**：[OpfsOffload 的实现](https://github.com/utooland/tokio-fs-ext/tree/master/src/fs/wasm/offload)不仅解决了 JS 对象在 Rust 下不满足线程安全的问题，也可以以极少的侵入性来扩展 `turbo-tasks-fs` 的文件系统。当检测到文件更改时，事件也会通过此层传播到在 WASM 环境中运行的 Turbopack 引擎。
3.  **增量编译**：Turbopack 的架构建立在响应式图之上。当它接收到文件更改事件时，它仅使依赖图中受影响的部分失效。这触发了仅针对更改的模块及其依赖项的重新计算（重建），从而实现极快的更新，类似于原生开发环境中的热模块替换（HMR）。

这种架构确保了 `@utoo/web` 即使对于完全在浏览器中运行的大型项目也能提供响应迅速的开发体验。

---

## 快速上手指南

启动项目主要涉及四个步骤。参考 `examples/utooweb-demo` 或在线体验 [`utoo-repl`](https://utoo-repl.vercel.app)。

### 1. 实例化项目

创建 `Project` 实例，配置 Worker 和 Service Worker。

```typescript
import { Project as UtooProject } from "@utoo/web";

const project = new UtooProject({
    // 文件系统中的项目根目录。
    cwd: "/utooweb-demo",

    // 核心功能 Worker 脚本 URL。
    workerUrl: `${location.origin}/worker.js`,

    // 重度任务 Worker 脚本 URL。
    threadWorkerUrl: `${location.origin}/threadWorker.js`,

    // Webpack loaders Worker 脚本 URL。
    loaderWorkerUrl: `${location.origin}/loaderWorker.js`,
    
    // 预览 Service Worker 配置。
    serviceWorker: {
        url: `${location.origin}/serviceWorker.js`,
        scope: "/preview", // Service Worker 控制的路径。
    },
    // 运行 webpack loaders 的 ImportMap
    loadersImportMap: {
      // 接受 umd 脚本 url 或脚本内容字符串
      "xyzLoader": "https://x.y.z.js"
    }
});
```

### 2. 安装 Service Worker

要启用预览功能，您必须注册并安装 Service Worker。

```typescript
await project.installServiceWorker();
```

### 3. 安装依赖

`@utoo/web` 可以从 `package-lock.json` 安装依赖，这和 `npm install` 过程一致。我们很快将会支持无需 `package-lock.json` 就能安装依赖。

项目 `node_modules` 中的依赖包实际上是指向全局共享存储的逻辑链接。这意味着在同一浏览器域名下的不同 project 之间，可以共享同名且同版本的依赖，无需重复下载。这种机制类似于 `pnpm` 的存储策略。

这种设计具有以下优势：
1. **节省存储空间**：相同版本的依赖包在 OPFS 中仅存储一份，避免了冗余占用。
2. **加速项目初始化**：创建新项目或切换项目时，若依赖包已存在于全局存储中，可直接复用，实现秒级安装。
3. **减少网络流量**：常用依赖包只需下载一次，后续项目即可直接使用，大幅降低网络开销。
4. **跨标签页复用**：即使开启新的浏览器标签页，只要处于同一域名下，依赖均可以直接复用。

```typescript
// 将您的 package-lock.json 作为 JSON 对象导入。
import { packageLock } from "../packageLock";

await project.install(JSON.stringify(packageLock));
```

### 4. 写入项目文件

环境设置好后，您现在可以将源文件写入真实文件系统。

```typescript
// 一个包含文件路径及其内容的对象。
import { demoFiles } from "../demoFiles";

await project.mkdir("src");

for (const filePath in demoFiles) {
    const content = demoFiles[filePath];
    await project.writeFile(filePath, content);
}
```

### 5. 创建构建配置

在构建项目之前，您需要在项目的根目录中提供一个名为 `utoopack.json` 的构建配置文件。该文件告诉 `@utoo/web` 如何打包您的应用程序，指定入口点和其他构建选项。

一个典型的配置如下所示：

```json
{
  "entry": [
    {
      "import": "./src/index.tsx",
      "name": "main" 
    }
  ],
  "output": {
    "path": "dist"
  },
  "module": {
    "rules": {
      "*.tsx": [ "xyzLoader" ]
    }
  }, 
  "stats": true
}
```

若要使用 loader，请将其添加至 `package.json` 的 `devDependencies` 中并安装，这与标准 Webpack 项目的依赖管理方式一致。此外，由于 `@utoo/web` 遵循 `loader-runner` 的机制与上下文来执行 loader，您还需要同时安装 `loader-runner`。

您可以像写入其他源文件一样，将此文件写入真实文件系统：

```typescript
await project.writeFile('utoopack.json', JSON.stringify(utoopackConfig, null, 2));
```

完成这些步骤后，您的项目就完全初始化并准备好进行交互了。

---

## API 参考

### `new UtooProject(options)`

创建一个新的项目实例。

**选项:**

* `cwd` (string, 必需): 在真实文件系统中作为项目根目录的绝对路径（例如 `/my-app`）。
* `workerUrl` (string, 可选): 指定 `Project` 实例核心逻辑实际运行的 Worker 线程的 URL。您在主线程中与之交互的 `Project` 对象是一个代理，它将所有核心任务（如文件系统操作）委托给此 Worker。这种架构是保持 UI 响应的关键。
* `threadWorkerUrl` (string, 必需): 指定一个专用于处理 CPU 密集型任务（如打包和编译）的独立 Worker 线程的 URL。这将重量级的构建过程与 `Project` 的主要逻辑 Worker 隔离开来。
* `loaderWorkerUrl` (string, 必需): 指定一个专用于处理 webpack 加载器的独立 Worker 线程的 URL。
* `serviceWorker` (object, 可选):
  * `url` (string, 必需): Service Worker 脚本的 URL。
  * `scope` (string, 必需): Service Worker 将拦截请求的 URL 范围。这是您预览环境的基路径。
* `loadersImportMap`（对象，可选）：用于在 @utoo/web 中打包时运行 webpack 加载器的加载器导入映射。键是加载器的名称，值可以是 UMD 字符串 URL 或 UMD 内容字符串。加载器将在 Web Worker 池中并行执行。

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

### 预览功能

#### `project.installServiceWorker()`

注册并激活构造函数中定义的 Service Worker。这对于预览功能至关重要。

#### `project.install(packageLockJsonString)`

根据 `package-lock.json` 填充 `node_modules` 目录。

* `packageLockJsonString` (string): `package-lock.json` 文件的 JSON 字符串。

#### `project.build()`

在线程 Worker 中触发构建过程。它会从项目根目录读取 `utoopack.json` 的构建配置，并根据该配置运行打包器。

* 返回: `Promise<void>` - 当构建完成时，Promise 会解决。如果构建失败，则会拒绝。

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

设置 `@utoo/web` 项目的一个关键部分是创建您传递给 `UtooProject` 构造函数的 Worker 脚本。正如在 `utooweb-demo` 示例中所见，这些文件的内容非常少。它们的目的是简单地从 `@utoo/web` 库本身加载必要的 Worker 逻辑。

您需要在项目的源代码中创建三个文件，然后由您的打包器（例如 Webpack、Vite）编译成最终传递给构造函数的 URL。

#### 1. 项目主 Worker (`worker.ts`)

此文件为主项目 Worker 提供逻辑，该 Worker 处理文件系统操作和其他核心任务。

```typescript
// src/worker.ts
import "@utoo/web/esm/worker";
```

#### 2. 线程 Worker (`threadWorker.ts`)

此文件为构建 Worker 提供逻辑，该 Worker 处理 CPU 密集型任务，如打包。

```typescript
// src/threadWorker.ts
import "@utoo/web/esm/threadWorker";
```

#### 3. 服务 Worker (`serviceWorker.ts`)

此文件为 Service Worker 提供逻辑，该 Worker 从真实文件系统提供预览。

```typescript
// src/serviceWorker.ts
import "@utoo/web/esm/serviceWorker";
```

#### 4. 加载器 Worker (`loaderWorker.ts`)

此文件为加载器 Worker 提供逻辑，该 Worker 处理 webpack 加载器。

```typescript
// src/loaderWorker.ts
import "@utoo/web/esm/loaderWorker";
```

您的构建设置应配置为将这些文件输出到主应用程序可以访问的位置，以便您可以将其 URL 提供给 `UtooProject` 构造函数。

## 注意

* 由于当前 Rust 上默认的内存分配器 [`dlmalloc`](https://github.com/alexcrichton/dlmalloc-rs) 在多线程 `wasm` 上性能不够理想，我们将 [`mimalloc`](https://github.com/microsoft/mimalloc) 移植到了 wasm32-unknown-unknown 平台，以支持开启 CPU 核心数量的线程来运行构建。因此在浏览器环境和在操作系统环境，构建的性能差异十分微小。
* 未来我们也会在浏览器中支持 [`HMR`](https://webpack.js.org/concepts/hot-module-replacement/) 功能;
* turbopack 的部分高级功能如[`持久化缓存`](https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopackPersistentCaching)，目前也在计划之中，未来会在浏览器内直接支持。

