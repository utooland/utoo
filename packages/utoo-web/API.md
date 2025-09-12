# [`@utoo/web`]## Quick Start Guide

Getting a project up and running involves four main steps, as demonstrated in `examples/utooweb-demo`. You can also experience the demo online at [`utoo-repl`](https://utoo-repl.vercel.app/).tps://www.npmjs.com/package/@utoo/web) API Documentation

`@utoo/web` is a powerful library that enables you to run a complete web development environment, including a virtual file system, dependency management, and a build process, entirely within the browser. It deeply integrates [`utoopack`](https://github.com/utooland/utoo), a new builder based on [`Rust`](https://www.rust-lang.org/) and [`turbopack`](https://nextjs.org/docs/app/api-reference/turbopack), and leverages modern web technologies like Web Workers, Service Workers, and the Origin Private File System (OPFS) to provide a seamless and fast development experience without a backend server.

## Core Concepts

Before diving into the API, it's important to understand the four main components that make `@utoo/web` work:

1.  **Virtual File System**: The entire project, including source code and `node_modules`, lives in the browser's Origin Private File System (OPFS). The `Project` class provides a Node.js-like `fs` interface to interact with it.
2.  **Project Main Worker**: The core logic of the `Project` instance runs within its own Web Worker. The `Project` object you interact with in the main thread is a proxy that delegates all core tasks (like file system operations) to this worker. This architecture is key to keeping the UI responsive.
3.  **Thread Worker**: Heavy tasks like bundling and compilation are offloaded to a dedicated Web Worker. This ensures the main UI thread remains responsive, even during a build. We have ported [`tokio`](https://github.com/utooland/tokio) to the browser to take full advantage of multi-core CPUs for improved performance. The **Thread Worker** will be completely taken over by the tokio runtime.
4.  **Service Worker**: A Service Worker acts as a local server. It intercepts requests from the preview `iframe`, reads the corresponding files from the OPFS, and serves them back, allowing you to preview your built application.

---

## Quick Start Guide

Getting a project up and running involves four main steps, as demonstrated in `examples/utooweb-demo`. You can also experience the demo online at [`utoo-repl`](https://utoo-repl.vercel.app).

### 1. Instantiate the Project

First, create an instance of the `Project` class. It requires configuration for the project's root directory, the thread worker, and the service worker.

```typescript
import { Project as UtooProject } from "@utoo/web";

const project = new UtooProject({
    // The root directory for your project inside the virtual file system.
    cwd: "/utooweb-demo",

    // The URL of the worker script that manages the virtual file system and other core functionalities.
    workerUrl: `${location.origin}/worker.js`,

    // The URL of the worker script that handles heavy tasks.
    threadWorkerUrl: `${location.origin}/threadWorker.js`,
    
    // Configuration for the preview service worker.
    serviceWorker: {
        url: `${location.origin}/serviceWorker.js`,
        scope: "/preview", // The path the service worker will control.
    },
});
```

### 2. Install the Service Worker

To enable the preview functionality, you must register and install the service worker.

```typescript
await project.installServiceWorker();
```

### 3. Install Dependencies

`@utoo/web` can install dependencies from a `package-lock.json` file, which is consistent with the `npm install` process. We will soon support installing dependencies without a `package-lock.json`.

```typescript
// Import your package-lock.json as a JSON object.
import { packageLock } from "../packageLock";

await project.install(JSON.stringify(packageLock));
```

### 4. Write Project Files

With the environment set up, you can now write your source files to the virtual file system.

```typescript
// An object containing file paths and their content.
import { demoFiles } from "../demoFiles";

await project.mkdir("src");

for (const filePath in demoFiles) {
    const content = demoFiles[filePath];
    await project.writeFile(filePath, content);
}
```

### 5. Create Build Configuration

Before you can build the project, you need to provide a build configuration file named `utoopack.json` in the project's root directory. This file tells `@utoo/web` how to bundle your application, specifying entry points and other build options.

A typical configuration looks like this:
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
  "stats": true
}
```

You would write this file to the virtual file system just like any other source file:
```typescript
await project.writeFile('utoopack.json', JSON.stringify(utoopackConfig, null, 2));
```

After these steps, your project is fully initialized and ready for interaction.

---

## API Reference

### `new UtooProject(options)`

Creates a new project instance.

**Options:**

*   `cwd` (string, required): The absolute path that will serve as the root of the project in the virtual file system (e.g., `/my-app`).
*   `workerUrl` (string, optional): Specifies the URL of the Worker thread where the `Project` instance's core logic actually runs. The `Project` object you interact with in the main thread is a proxy that delegates all core tasks (like file system operations) to this Worker. This architecture is key to keeping the UI responsive.
*   `threadWorkerUrl` (string, required): Specifies the URL of a separate Worker thread dedicated to handling CPU-intensive tasks like bundling and compiling. This isolates the heavy build process from the `Project`'s main logic worker.
*   `serviceWorker` (object, optional):
    *   `url` (string, required): The URL to the service worker script.
    *   `scope` (string, required): The URL scope that the service worker will intercept requests for. This is the base path for your preview environment.

### File System Methods

These methods are asynchronous and mimic the Node.js `fs` API.

#### `project.writeFile(path, content)`

Writes content to a file in the virtual file system. If the file doesn't exist, it will be created.

*   `path` (string): The absolute path to the file (e.g., `/src/index.js`).
*   `content` (string | Buffer): The content to write.

#### `project.readFile(path, encoding)`

Reads the content of a file.

*   `path` (string): The path to the file.
*   `encoding` (string, optional): The encoding of the file (e.g., `'utf8'`). If not provided, it returns a Buffer.

#### `project.readDir(path)`

Reads the contents of a directory.

*   `path` (string): The path to the directory.
*   Returns: `Promise<string[]>` - An array of file and directory names.

#### `project.mkdir(path)`

Creates a new directory.

*   `path` (string): The path to the directory to be created.

#### `project.rm(path, options)`

Removes a file or directory.

*   `path` (string): The path to the file or directory to be removed.
*   `options` (object, optional):
    *   `recursive` (boolean): If `true`, performs a recursive directory removal. Defaults to `false`.

#### `project.rmdir(path)`

Removes a directory.

*   `path` (string): The path to the directory to be removed.

### Preview Functionality

#### `project.installServiceWorker()`

Registers and activates the service worker defined in the constructor. This is essential for the preview functionality.

#### `project.install(packageLockJsonString)`

Populates the `node_modules` directory based on a `package-lock.json`.

*   `packageLockJsonString` (string): A JSON string of your `package-lock.json` file.

#### `project.build()`

Triggers the build process in the thread worker. It reads the build configuration from `utoopack.json` in the project's root and runs the bundler based on that configuration.

*   Returns: `Promise<void>` - The promise resolves when the build is complete. It will reject if the build fails.

---

## Example Workflow: Building and Previewing

The `utooweb-demo` shows a complete workflow for editing, building, and previewing.

1.  **Editing**: A file is read using `project.readFile()` and displayed in an editor. When the content changes, `project.writeFile()` is called (often with a debounce) to save the changes back to the OPFS.

    ```typescript
    // In useFileContent.ts
    const content = await project.readFile(filePath, "utf8");
    // ...
    await project.writeFile(selectedFilePath, newContent);
    ```

2.  **Building**: The user clicks a "Build" button, which calls `project.build()`.

    ```typescript
    // In useBuild.ts
    setIsBuilding(true);
    try {
        await project.build();
        // Build succeeded
    } catch (e) {
        // Build failed
    } finally {
        setIsBuilding(false);
    }
    ```

3.  **Processing Build Output**: After a successful build, the application reads the build output (e.g., `dist/stats.json`) to find the generated asset files (`.js`, `.css`). It then generates an `index.html` that includes these assets.

    ```typescript
    // In useBuild.ts
    const statsContent = await project.readFile("dist/stats.json", "utf8");
    const stats = JSON.parse(statsContent);
    // ... logic to generate HTML with correct script/link tags ...
    await project.writeFile("dist/index.html", generatedHtml);
    ```

4.  **Previewing**: The `Preview` component contains an `iframe` whose `src` points to the entry point within the service worker's scope (e.g., `/preview/dist/index.html`). When the build completes, the `iframe` reloads from the Service Worker with the newly generated artifact files from OPFS.

This cycle provides a fast and interactive development loop, all running locally in the user's browser.

---

## Server Configuration: COOP & COEP Headers

To use `@utoo/web`, your development server must serve the application with specific HTTP headers to create a **cross-origin isolated environment**. This is a security requirement browsers enforce to enable powerful features like `SharedArrayBuffer`, which are essential for the multi-threaded performance of the underlying WebAssembly components.

You must configure your server to send the following two headers:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

#### Example: `webpack-dev-server`

If you are using `webpack-dev-server`, you can add these headers in your `webpack.config.js` file as shown in the `utooweb-demo`:

```javascript
// webpack.config.js
module.exports = {
  // ... other config
  devServer: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    // ... other devServer options
  },
};
```

**Important**: Without these headers, the browser will not enable the necessary features, and `@utoo/web` will fail to initialize. This configuration is required for any server hosting your application, not just `webpack-dev-server`.

---

## Setting Up the Worker Scripts

A key part of setting up a `@utoo/web` project is creating the worker scripts that you pass to the `UtooProject` constructor. As seen in the `utooweb-demo` example, the content of these files is minimal. Their purpose is to simply load the necessary worker logic from the `@utoo/web` library itself.

You will need to create three files in your project's source, which will then be compiled by your bundler (e.g., Webpack, Vite) into the final URLs passed to the constructor.

#### 1. Project Main Worker (`worker.ts`)

This file provides the logic for the main project worker, which handles file system operations and other core tasks.

```typescript
// src/worker.ts
import "@utoo/web/esm/worker";
```

#### 2. Thread Worker (`threadWorker.ts`)

This file provides the logic for the build worker, which handles CPU-intensive tasks like bundling.

```typescript
// src/threadWorker.ts
import "@utoo/web/esm/threadWorker";
```

#### 3. Service Worker (`serviceWorker.ts`)

This file provides the logic for the service worker, which serves the preview from the virtual file system.

```typescript
// src/serviceWorker.ts
import "@utoo/web/esm/serviceWorker";
```

Your build setup should be configured to output these files to a location that your main application can access, so you can provide their URLs to the `UtooProject` constructor.

## Notes
* Due to the current default memory allocator for Rust, [`dlmalloc`](https://github.com/alexcrichton/dlmalloc-rs), having less than ideal performance on multi-threaded `wasm`, we are currently trying to support [`mimalloc`](https://github.com/microsoft/mimalloc) with reference to [`emscripten`](https://emscripten.org/docs/tools_reference/settings_reference.html#malloc)'s solution. Once successful, the build speed will be greatly improved;
* In the future, we will also support the [`HMR`](https://webpack.js.org/concepts/hot-module-replacement/) feature in the browser;
* Advanced features of turbopack such as [`webpack loaders`](https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopack#configuring-webpack-loaders), [`persistent caching`](https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopackPersistentCaching), are also in the plan and will be supported directly in the browser in the future.
