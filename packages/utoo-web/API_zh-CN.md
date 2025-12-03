# [`@utoo/web`](https://www.npmjs.com/package/@utoo/web) API 文档

`@utoo/web` 是一个功能强大的库，它允许您在浏览器中完整地运行一套 Web 开发环境，包括虚拟文件系统、依赖管理和构建流程。它深度整合了基于 [`Rust`](https://www.rust-lang.org/) 和 [`turbopack`](https://nextjs.org/docs/app/api-reference/turbopack) 的全新构建器 [`utoopack`](https://github.com/utooland/utoo)，并利用 Web Workers、Service Workers 和源私有文件系统（OPFS）等现代 Web 技术，提供无缝且快速的开发体验，无需后端服务器。

## 核心概念

在深入了解 API 之前，理解构成 `@utoo/web` 的四个主要组件非常重要：

1. **Virtual File System**：整个项目，包括源代码和 `node_modules`，都存在于浏览器的源私有文件系统（OPFS）中。`Project` 类提供了一个类似 Node.js `fs` 的接口来与其交互。
2. **Project Main Worker**：`Project` 实例的核心逻辑运行在它自己的 Web Worker 中。您在主线程中与之交互的 `Project` 对象实际上是一个代理，它将所有核心任务（如文件系统操作）委托给此 Worker。这种架构是保持 UI 响应流畅的关键。
3. **Thread Worker**：像打包和编译这样的重度任务被卸载到一个专用的 Web Worker 中。这确保了即使在构建过程中，主 UI 线程也能保持响应。我们将 [`tokio`](https://github.com/utooland/tokio) 移植到了浏览器上，以充分利用多核 CPU 提升性能。**Thread Worker** 将完全被 tokio runtime 接管。
4. **Service Worker**：Service Worker 充当本地服务器。它拦截来自预览 `iframe` 的请求，从 OPFS 中读取相应的文件，并将其提供回去，从而允许您预览构建好的应用程序。

---

## 快速上手指南

启动一个项目主要涉及四个步骤，如 `examples/utooweb-demo` 中所示。你也可以在 [`utoo-repl`](https://utoo-repl.vercel.app) 在线体验演示效果。

### 1. 实例化项目

首先，创建 `Project` 类的一个实例。它需要配置项目根目录、线程 Worker 和 Service Worker。

```typescript
import { Project as UtooProject } from "@utoo/web";

const project = new UtooProject({
    // 虚拟文件系统中项目的根目录。
    cwd: "/utooweb-demo",

    // 管理虚拟文件系统和其他核心功能的 Worker 脚本的 URL。
    workerUrl: `${location.origin}/worker.js`,

    // 处理重度任务的 Worker 脚本的 URL。
    threadWorkerUrl: `${location.origin}/threadWorker.js`,
    
    // 预览 Service Worker 的配置。
    serviceWorker: {
        url: `${location.origin}/serviceWorker.js`,
        scope: "/preview", // Service Worker 将控制的路径。
    },
    loadersImportMap: {
       // accept an umd script url or a script content string
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

```typescript
// 将您的 package-lock.json 作为 JSON 对象导入。
import { packageLock } from "../packageLock";

await project.install(JSON.stringify(packageLock));
```

### 4. 写入项目文件

环境设置好后，您现在可以将源文件写入虚拟文件系统。

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

您可以像写入其他源文件一样，将此文件写入虚拟文件系统：

```typescript
await project.writeFile('utoopack.json', JSON.stringify(utoopackConfig, null, 2));
```

完成这些步骤后，您的项目就完全初始化并准备好进行交互了。

---

## API 参考

### `new UtooProject(options)`

创建一个新的项目实例。

**选项:**

* `cwd` (string, 必需): 在虚拟文件系统中作为项目根目录的绝对路径（例如 `/my-app`）。
* `workerUrl` (string, 可选): 指定 `Project` 实例核心逻辑实际运行的 Worker 线程的 URL。您在主线程中与之交互的 `Project` 对象是一个代理，它将所有核心任务（如文件系统操作）委托给此 Worker。这种架构是保持 UI 响应的关键。
* `threadWorkerUrl` (string, 必需): 指定一个专用于处理 CPU 密集型任务（如打包和编译）的独立 Worker 线程的 URL。这将重量级的构建过程与 `Project` 的主要逻辑 Worker 隔离开来。
* `serviceWorker` (object, 可选):
  * `url` (string, 必需): Service Worker 脚本的 URL。
  * `scope` (string, 必需): Service Worker 将拦截请求的 URL 范围。这是您预览环境的基路径。
* `loadersImportMap`（对象，可选）：用于在 @utoo/web 中打包时运行 webpack 加载器的加载器导入映射。键是加载器的名称，值可以是 UMD 字符串 URL 或 UMD 内容字符串。加载器将在 Web Worker 池中并行执行。

### 文件系统方法

这些方法是异步的，并模仿了 Node.js `fs` API。

#### `project.writeFile(path, content)`

将内容写入虚拟文件系统中的文件。如果文件不存在，将会被创建。

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

3. **处理构建输出**: 构建成功后，应用程序读取构建输出（例如 `dist/stats.json`）以查找生成的资源文件（`.js`、`.css`）。然后它会生成一个包含这些资源的 `index.html`。

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

此文件为 Service Worker 提供逻辑，该 Worker 从虚拟文件系统提供预览。

```typescript
// src/serviceWorker.ts
import "@utoo/web/esm/serviceWorker";
```

您的构建设置应配置为将这些文件输出到主应用程序可以访问的位置，以便您可以将其 URL 提供给 `UtooProject` 构造函数。

## 注意

* 由于当前 Rust 上默认的内存分配器 [`dlmalloc`](https://github.com/alexcrichton/dlmalloc-rs) 在多线程 `wasm` 上性能不够理想，我们目前正在尝试参考 [`emscripten`](https://emscripten.org/docs/tools_reference/settings_reference.html#malloc) 的方案支持 [`mimalloc`](https://github.com/microsoft/mimalloc)，一旦成功，构建速度将会有大幅提升；
* 未来我们也会在浏览器中支持 [`HMR`](https://webpack.js.org/concepts/hot-module-replacement/) 功能;
* turbopack 的部分高级功能如[`持久化缓存`](https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopackPersistentCaching)，目前也在计划之中，未来会在浏览器内直接支持。

