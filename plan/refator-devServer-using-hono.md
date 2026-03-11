# 使用 Hono 重构 Dev Server 计划

## 一、当前实现理解

### 1.1 入口与调用链

- **对外 API**：`packages/pack/src/index.ts` 导出 `serve`，来自 `./commands/dev`。
- **调用方**：`pack-cli` 的 `dev` 命令通过 `utooPack.serve(projectOptions, projectPath, rootPath, serverOptions)` 启动开发服务器。
- **内部流程**：`serve()` → `serveInternal()` → `startServer()`；在 `startServer()` 里先起 Node.js `http`/`https` 服务器，在 `listening` 回调中再执行 `initialize()`，得到 `requestHandler` / `upgradeHandler` 并 resolve 占位用的 Promise。

### 1.2 当前 HTTP 处理

- **协议**：仅处理 `GET` 和 `HEAD`，其它方法应返回 405（当前实现存在 bug：设置 405 后未 `return`，会继续走到静态逻辑）。
- **路径**：根据 `config.output.publicPath` 做规范化与前缀剥离（`normalizedPublicPath`、`pathHasPrefix`、`removePathPrefix`），再得到用于静态文件的 `requestPath`。
- **静态资源**：使用 `send` 包，以 `distRoot`（`output.path` 解析结果）为根目录，对 `requestPath` 做静态文件服务；目录请求会触发 `directory` 事件并 reject（相当于 404）。
- **错误**：未找到或异常时返回 404，并设置 `Cache-Control: private, no-cache, ...`；其它异常 500。

### 1.3 WebSocket / 升级

- 在 `startServer()` 里对 `server` 注册 `on("upgrade", ...)`，委托给 `upgradeHandler`。
- `initialize()` 提供的 `upgradeHandler`：若 URL 包含 `turbopack-hmr` 则交给 `hotReloader.onHMR(req, socket, head)`（`core/hmr.ts` 使用 `ws` 的 `WebSocketServer({ noServer: true })`），否则 `socket.end()`。
- 与 HMR 的对接是 Node 原生 `IncomingMessage` + `Duplex` + `Buffer`，不依赖 Hono。

### 1.4 服务器创建与生命周期

- **创建**：根据 `serverOptions.selfSignedCertificate` 选择 `https.createServer(...)` 或 `http.createServer(requestListener)`。
- **端口**：支持 `EADDRINUSE` 时自动重试（最多 10 次，port+1）。
- **关闭**：监听 SIGINT/SIGTERM，关闭 server、`closeUpgraded()`（即 `hotReloader.close()`）。

### 1.5 已有依赖

- `packages/pack/package.json` 中已有 `hono`、`@hono/node-server`，当前未在 dev 流程中使用。
- 静态目前用 `send`，可考虑用 Hono 生态的静态中间件替代。

### 1.6 关键依赖与工具函数说明

- **`send`（npm 包）**：Node 下常用的**静态文件流式发送库**。在 `serveStatic()` 里使用：根据请求的 path 和选项（如 `root`）从磁盘读文件并流式 pipe 到 `ServerResponse`。会处理 ETag、Range、目录请求等；当前对目录请求监听 `directory` 事件并 reject（等价于不提供目录列表，当 404 处理）。重构后用 Hono 的 `serveStatic` 可替代，即可移除 `send` 依赖。

- **`parsePath(pathStr)`**：把一段 URL 路径字符串拆成 `{ pathname, query, hash }`，只做字符串分割（不解析成 URL 对象）。用于在判断“路径前缀”时只看 pathname，忽略 `?` 和 `#` 后面的部分。

- **`pathHasPrefix(pathStr, prefix)`**：判断 `pathStr` 的 pathname 部分是否等于 `prefix` 或以 `prefix + "/"` 开头。依赖 `parsePath`。用于判断当前请求 path 是否带有配置的 `publicPath` 前缀（例如 `/assets`），以便决定是否要做前缀剥离。

- **`removePathPrefix(pathStr, prefix)`**：若 path 带有 `prefix` 则去掉该前缀，得到剩余路径（保证以 `/` 开头）；否则原样返回。用于：请求为 `/assets/main.js`、publicPath 为 `/assets` 时，得到 `/main.js`，再在 `distRoot` 下找文件。

- **`normalizedPublicPath(publicPath)`**：把配置里的 `output.publicPath`（可能是 `"/assets"`、`"assets/"`、或绝对 URL）规范成统一格式：去掉首尾斜杠、绝对 URL 用 `URL.canParse` 判断并原样返回，否则变成 `/xxx`。得到的是用于 `pathHasPrefix` / `removePathPrefix` 的“标准前缀”，避免因配置写法不同导致匹配失败。

---

## 二、重构目标

- 将 **HTTP 请求处理** 从手写 `IncomingMessage`/`ServerResponse` + `send` 改为基于 **Hono** 的路由与中间件。
- **功能一致**：对外行为与现有一致（`serve()` 对外签名、静态与 HMR、HTTPS、端口、进程清理等），pack-cli 无需改动。**不必保留原有封装**：底层实现可按 Hono/node-ws 重写，可抛弃 `startServer`/`initialize`/`requestHandler`/`upgradeHandler` 等历史结构，避免被旧实现束缚。
- **直接使用 Hono Node 适配器的完整能力**：用 `@hono/node-server` 的 **`serve({ fetch: app.fetch, port, hostname, createServer?, serverOptions? })`** 负责“创建 server + 监听”的完整启动流程（通过选项支持用户自定义端口/hostname、HTTP 与 HTTPS 及证书配置），用 **`@hono/node-ws`** 的 **`createNodeWebSocket` + `injectWebSocket(server)`** 负责 WebSocket 路由与升级，不再手写 `createServer`、`getRequestListener`、`server.on('upgrade')`。参考：[Node.js - Hono](https://hono.dev/docs/getting-started/nodejs)、[WebSocket Helper](https://hono.dev/docs/helpers/websocket)、[@hono/node-ws](https://github.com/honojs/middleware/tree/main/packages/node-ws)。
- **按新框架做职责划分与分层**：在保持行为的前提下，按**代码逻辑**做分层（入口/编排、初始化与应用组装、启动用官方 `serve()` + `injectWebSocket()`），边界清晰且复用官方适配器。
- **文件与迁移**：新实现写在 **`dev-hono.ts`**，与 `dev.ts` 同级（`packages/pack/src/commands/dev-hono.ts`）。**暂不删除** `dev.ts` 中原有代码；待确认 Hono 版功能完整后，再由维护者手动删除旧实现并切换入口（如将 `index.ts` 的 `serve` 改为从 `dev-hono` 导出）。

---

## 三、职责划分与分层设计（建议，按代码逻辑）

采用 **Hono Node 适配器的完整能力**：用 `@hono/node-server` 的 `serve()` 负责启动服务，用 `@hono/node-ws` 负责 WebSocket 升级与路由，不再手写 `createServer` + `getRequestListener` + `on('upgrade')`。

- **入口 / 编排**：对外仍暴露 `serve(...)`；内部可重新组织，不必沿用 `serveInternal` 等命名。解析入参、合并 devServer 配置、可选证书生成，然后按新流程组 App、启动 server。

- **准备与组装**：在调用 `serve()` 前，准备好“开发期能力”（Turbopack 监听、HMR 广播等，可保留 HotReloader 核心逻辑或重写为更贴合 Hono 的形态）、以及 **Hono App**：静态路由 + 405 + HMR WebSocket 路由（`upgradeWebSocket` 内对接多连接广播与订阅）。不必保留 `initialize()` 返回 `requestHandler`/`upgradeHandler` 的旧接口；按“先准备好 app 与清理回调，再 `serve(app)` + `injectWebSocket(server)`”即可。

- **应用组装（路由与静态 + WebSocket）**：创建 Hono App 并注册：① GET/HEAD → `serveStatic`（`root`、`rewriteRequestPath`）；② 其它 method → 405；③ HMR WebSocket 路径 → `upgradeWebSocket`，在 onOpen/onClose/onMessage 中对接多连接管理与 Turbopack 订阅。输入为配置与“HMR 能力”（无论是否仍叫 HotReloader），输出为组装好的 App。

- **启动与生命周期**：调用 **`serve({ fetch: app.fetch, port, hostname, createServer?, serverOptions? })`** 得到 `server`，再 **`injectWebSocket(server)`**；进程信号时 `server.close()` 并执行清理（如关闭 Turbopack 监听、关闭 HMR 相关资源），实现优雅退出。

以上为**逻辑分层建议**，具体命名与是否保留 `initialize`/`serveInternal` 等可按实现需要决定，以功能一致、结构清晰、少背历史包袱为准。

---

## 四、技术方案要点

### 4.1 使用 @hono/node-server 的 `serve()` 启动服务

- **必须使用“选项对象”形式**（[Node.js - Hono](https://hono.dev/docs/getting-started/nodejs)）：`serve({ fetch: app.fetch, port, hostname, createServer?, serverOptions? })`，不能只传 `serve(app)`。原因：需要支持用户自定义端口与 hostname、以及通过选项区分启动 **HTTP** 还是 **HTTPS**；若为 HTTPS，还需传入证书配置（Node.js 适配器的 `Options` 支持上述能力，可直接实现）。
- **选项约定**：
  - **port**、**hostname**：来自用户/配置（如 `serverOptions.port`、`serverOptions.hostname` 或 devServer 配置），用于 `serve({ fetch: app.fetch, port, hostname, ... })`。
  - **HTTP**：不传 `createServer` 时使用适配器默认的 `http.createServer`。
  - **HTTPS**：传 `createServer: require('https').createServer`，并传 **serverOptions**：`{ key, cert }`（证书内容；自签名时用 `selfSignedCertificate.key` / `selfSignedCertificate.cert` 读出的字符串或 Buffer），与官方文档中 [encrypted http2](https://hono.dev/docs/getting-started/nodejs#encrypted-http2) 的用法一致（适配器类型为 `createHttpsOptions`：`serverOptions` 即 Node 的 `https.ServerOptions`）。
- **端口占用**：使用专门查找可用端口的库（如 [get-port](https://github.com/sindresorhus/get-port) 或 [get-port-please](https://github.com/unjs/get-port-please)）在调用 `serve()` 前解析出可用端口（可指定首选 port，若被占用则自动选下一个），再将得到的 port 传入 `serve({ ..., port })`，避免手写 EADDRINUSE 重试。next.js 等项目中有使用先例。
- **优雅退出**：参考官方示例，在得到 `server` 后注册信号处理；本项目中还需在退出前调用 `hotReloader.close()`。

```ts
const server = serve(app)

// graceful shutdown
process.on('SIGINT', () => {
  server.close()
  process.exit(0)
})
process.on('SIGTERM', () => {
  server.close((err) => {
    if (err) {
      console.error(err)
      process.exit(1)
    }
    process.exit(0)
  })
})
```

### 4.2 Hono 路由与静态

- **仅允许 GET/HEAD**：在 Hono 里用 `app.get("/*", ...)` 处理静态即可，GET 路由会同时处理 HEAD；其它方法用 `app.all("*", ...)` 统一返回 405（并设置 `Allow: GET, HEAD`）。
- **静态与路径重写**：使用 **`serveStatic`**（[Serve static files](https://hono.dev/docs/getting-started/nodejs#serve-static-files)）从 `@hono/node-server/serve-static`，配置：
  - `root`：即当前的 `distRoot`（`path.resolve(projectPath, output.path || "./dist")`）；
  - `rewriteRequestPath`：当 `publicPath` 存在且非 `runtime` 且非绝对 URL 时，用现有的 `normalizedPublicPath` + `pathHasPrefix`/`removePathPrefix` 逻辑，把请求 path 重写为“去掉 publicPath 前缀”的 path；否则返回原 path。
- **404 / 错误**：404 保持简单、符合 Hono 默认即可；未捕获异常可在 Hono 的全局错误处理或中间件中返回 500。

路径工具函数（`parsePath`、`pathHasPrefix`、`removePathPrefix`、`normalizedPublicPath`）保留在 `dev.ts` 内使用，**不对外导出**；若抽到独立 util 文件也仅限包内使用、不从包入口导出。

### 4.3 WebSocket：使用 @hono/node-ws

- 使用 **[@hono/node-ws](https://github.com/honojs/middleware/tree/main/packages/node-ws)** 与 Hono 的 [WebSocket Helper](https://hono.dev/docs/helpers/websocket) 统一处理 WebSocket：
  - 在应用组装时：`const { injectWebSocket, upgradeWebSocket } = createNodeWebSocket({ app })`，对 HMR 路径（如包含 `turbopack-hmr` 的路径）注册 `app.get('/...', upgradeWebSocket((c) => ({ onMessage, onClose, onOpen, onError })))`，在回调内对接现有 HotReloader 的逻辑（将 ws 的 onMessage/onClose 等桥接到 `hotReloader` 的发送/关闭）。
  - 在 `serve({ fetch: app.fetch, port, hostname, ... })` 得到 `server` 后，调用 **`injectWebSocket(server)`**，将 WebSocket 升级注入到同一 server。
- **多实例（多连接）**：现有 HotReloader 已支持多客户端——用 `clients = new Set<WebSocket>()` 与 `clientStates = new WeakMap<WebSocket, ClientState>()` 维护多个连接，`send()` 会向所有 client 广播。**为何用 Set 而非 WeakSet**：需要遍历所有连接做广播（`for (const client of clients) { client.send(...) }`），而 **WeakSet 不可迭代**，无法枚举成员，因此必须用 Set；只要在 **onClose** 时正确 `clients.delete(client)`，该引用即释放，不会影响 GC。`clientStates` 已用 **WeakMap**，key 为 ws 时本身不阻止 ws 被回收，合适。迁移到 `upgradeWebSocket` 时需保持该语义：每个新连接在 **onOpen** 时注册到同一 `clients`/`clientStates`，在 **onClose** 时从 set/state 中移除，**onMessage** 走同一套订阅与消息分发逻辑。这样多 tab、多窗口同时连同一 HMR 端点时，行为与现有一致；若 HotReloader 当前仅暴露 `onHMR(req, socket, head)`，可抽一层“按 ws 实例注册/注销/收消息”的接口供 `upgradeWebSocket` 回调调用。
- 若现有 `hotReloader.onHMR(req, socket, head)` 与 `ws` 的 `WebSocketServer({ noServer: true }).handleUpgrade` 强耦合，可在 `upgradeWebSocket` 的回调中拿到连接后的 `ws`，再通过上述“按连接注册”的接口适配到 HotReloader，或保留一层薄适配器。

### 4.4 依赖与兼容

- 使用现有 `hono`、`@hono/node-server`；静态使用子路径 **`@hono/node-server/serve-static`**。
- **新增依赖**：**`@hono/node-ws`**（[middleware/packages/node-ws](https://github.com/honojs/middleware/tree/main/packages/node-ws)），用于 Node 下 Hono 的 WebSocket 路由与 `injectWebSocket(server)`。
- 用 Hono 的 `serveStatic` 替代 `send` 后，可移除 `send` 依赖。

---

## 五、实施步骤（建议）

目标为**功能一致**，流程与命名可按 Hono/node-ws 重新设计，不必拘泥于现有 `serveInternal`/`initialize`/`startServer` 等封装。

- **代码位置**：在 **`packages/pack/src/commands/dev-hono.ts`** 中实现新逻辑（与 `dev.ts` 同级）。不修改、不删除 `dev.ts` 中现有代码；等确认功能都提供后，再手动删除旧实现并将包入口（如 `index.ts` 的 `serve`）切到 `dev-hono`。

1. **准备配置与 HMR 能力**  
   - 解析入参与 devServer 配置，计算 `distRoot`、`publicPath`、端口等；如需 HTTPS 则准备证书。  
   - 准备“HMR 能力”：Turbopack 监听、多连接管理（Set + WeakMap 或等价）、广播与订阅逻辑。可保留现有 HotReloader 核心或按 `upgradeWebSocket` 的 onOpen/onMessage/onClose 重写为更贴合的形态，不必保留 `onHMR(req, socket, head)` 接口。

2. **组装 Hono App 并启动**  
   - 创建 App，注册静态（`serveStatic` + `rewriteRequestPath`）、405、以及 HMR 的 WebSocket 路由（`createNodeWebSocket` + `upgradeWebSocket`，在回调内对接上一步的 HMR 能力）。  
   - 使用 **get-port** / **get-port-please** 解析出可用 port，再调用 **`serve({ fetch: app.fetch, port, hostname, createServer?, serverOptions? })`** 得到 `server`，然后 **`injectWebSocket(server)`**。  
   - 注册 SIGINT/SIGTERM：`server.close()` 并执行清理（如关闭 Turbopack、释放 HMR 资源）。

3. **错误与边界**  
   - 405 / 404 按 Hono 默认即可；500 在 Hono 全局错误处理或中间件中处理并 log。

4. **测试与收尾**  
   - 跑现有 dev 相关测试、手动验证静态资源、HMR WebSocket、405、404、HTTPS、端口冲突重试。  
   - 新增依赖 `@hono/node-ws`；在确认 `dev-hono` 功能完整前不删除 `dev.ts`、不移除 `send`；确认后再手动删除旧实现、切换入口，并视情况移除 `send`；保留 `pipeToNodeResponse` 等仍被别处使用的工具。

---

## 六、可选后续

- 若后续需要 dev 下的 API 路由（如 `/api/...`），可在 Hono 中直接加路由，与静态、405 共存。
- 路径规范化与重写逻辑可抽成包内 util 便于单测，但不从包入口导出。

---

## 七、风险与注意

- **HMR 与 @hono/node-ws 的对接**：现有 `hotReloader.onHMR(req, socket, head)` 使用 `ws` 的 `WebSocketServer({ noServer: true }).handleUpgrade`；改用 `upgradeWebSocket` 后需在 onMessage/onClose 等回调中把事件桥接到 HotReloader 的发送与关闭逻辑，或为 HotReloader 增加一层“基于 ws 实例”的适配接口。
- **行为一致**：静态的目录请求、index 解析、MIME 等需与当前 `send` 行为对齐，避免前端资源路径或类型变化。
- **导出**：若保留 `RequestHandler`、`pipeToNodeResponse` 等类型/工具的对外的导出，接口不变，仅内部实现改为“Hono serve() + @hono/node-ws”。

**参考**：[Node.js - Hono](https://hono.dev/docs/getting-started/nodejs)、[WebSocket Helper](https://hono.dev/docs/helpers/websocket)、[@hono/node-ws](https://github.com/honojs/middleware/tree/main/packages/node-ws)。

以上为基于当前实现的初步计划，实现时可按实际类型与测试结果微调。

---

## 八、核心功能实现示例

以下为关键路径的示例代码，用于提前校验设计、规避接口与调用顺序问题；实现时需按实际类型与项目结构补全。**实现文件**：`packages/pack/src/commands/dev-hono.ts`（与 `dev.ts` 同级）；原 `dev.ts` 暂不删除，确认功能后再手动清理。

### 8.1 入口与 serve 选项（端口、hostname、HTTP/HTTPS）

```ts
import { serve as honoServe } from "@hono/node-server";
import getPort from "get-port"; // 或 get-port-please
import https from "https";
import fs from "fs";

// 对外签名保持不变
export function serve(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
  serverOptions?: StartServerOptions,
) {
  return runDev(options, projectPath, rootPath, serverOptions);
}

async function runDev(/* ... */) {
  const bundleOptions = resolveBundleOptions(/* ... */);
  const port =
    serverOptions?.port ?? bundleOptions.config?.devServer?.port ?? 3000;
  const hostname =
    serverOptions?.hostname ?? bundleOptions.config?.devServer?.host ?? "localhost";

  // 解析出可用端口后再启动，避免 EADDRINUSE 手写重试
  const actualPort = await getPort({ port, host: hostname });

  // app 由 createApp(distRoot, publicPath 等) 得到，见 8.2；需先注册 HMR 路由再 serve
  const opts: Parameters<typeof honoServe>[0] = {
    fetch: app.fetch,
    port: actualPort,
    hostname,
  };

  if (serverOptions?.https && serverOptions?.selfSignedCertificate) {
    opts.createServer = https.createServer;
    opts.serverOptions = {
      key: fs.readFileSync(serverOptions.selfSignedCertificate.key),
      cert: fs.readFileSync(serverOptions.selfSignedCertificate.cert),
    };
  }

  const server = honoServe(opts);
  injectWebSocket(server); // @hono/node-ws

  // 优雅退出：先清理 HMR/Turbopack（关闭订阅、清空 clients 等），再关 server
  const cleanup = () => {
    hmrCleanup();
    server.close(() => process.exit(0));
  };
  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);
}
```

### 8.2 组装 Hono App（静态 + 405 + 路径重写）

```ts
import { Hono } from "hono";
import { serveStatic } from "@hono/node-server/serve-static";
import { createNodeWebSocket } from "@hono/node-ws";
import path from "path";

function createApp(opts: {
  distRoot: string;
  normalizedPublicPath: string; // 已用 normalizedPublicPath(publicPath) 得到
}) {
  const app = new Hono();
  const { injectWebSocket, upgradeWebSocket } = createNodeWebSocket({ app });

  // 路径重写：请求 path 带 publicPath 前缀时去掉，再在 distRoot 下找文件
  const rewriteRequestPath = (reqPath: string) => {
    if (!opts.normalizedPublicPath) return reqPath;
    if (pathHasPrefix(reqPath, opts.normalizedPublicPath))
      return removePathPrefix(reqPath, opts.normalizedPublicPath);
    return reqPath;
  };

  // GET（含 HEAD）-> 静态；Hono 的 serveStatic 会处理 HEAD
  app.get(
    "/*",
    serveStatic({
      root: opts.distRoot,
      rewriteRequestPath,
    })
  );

  // 其它 method -> 405
  app.all("*", (c) => {
    return c.body(null, 405, { Allow: "GET, HEAD" });
  });

  return { app, injectWebSocket, upgradeWebSocket };
}
```

注意：Hono 路由有顺序，若 `app.all("*", 405)` 写在最前会拦截所有请求；应把 GET/HEAD 的静态路由放在前面，最后用 `app.all("*", ...)` 兜底 405。

### 8.3 HMR WebSocket 路由与多连接管理

```ts
// 在 createApp 内或外部，对 HMR 路径注册 upgradeWebSocket
const HMR_PATH = "/turbopack-hmr"; // 与 crates/pack-core 及 next turbopack 客户端一致

const clients = new Set<WebSocket>();
const clientStates = new WeakMap<WebSocket, ClientState>();

function registerHmrRoute(
  app: Hono,
  upgradeWebSocket: ReturnType<typeof createNodeWebSocket>["upgradeWebSocket"],
  project: Project // Turbopack project，用于 hmrEvents 等
) {
  app.get(
    HMR_PATH,
    upgradeWebSocket((c) => ({
      onOpen(_ev, ws) {
        clients.add(ws);
        clientStates.set(ws, {
          hmrPayloads: new Map(),
          turbopackUpdates: [],
          subscriptions: new Map(),
        });
        // 发送 TURBOPACK_CONNECTED、SYNC 等初始消息…
      },
      onMessage(ev, ws) {
        const data = JSON.parse(ev.data as string);
        // 根据 data.type: turbopack-subscribe / turbopack-unsubscribe 等
        // 调用 subscribeToHmrEvents(ws, data.path) 或 unsubscribeFromHmrEvents(ws, data.path)
      },
      onClose(_ev, ws) {
        clientStates.delete(ws);
        clients.delete(ws);
        // 若有 subscriptions，逐个 return
      },
      onError() {},
    }))
  );
}

// 广播到所有 client（与现有 send() 语义一致）
function broadcast(payload: HMR_ACTION_TYPES) {
  const str = JSON.stringify(payload);
  for (const client of clients) {
    try {
      client.send(str);
    } catch (_) {}
  }
}
```

设计要点：`createNodeWebSocket({ app })` 要在**注册路由之前**调用，且同一个 `app` 上先注册好所有使用 `upgradeWebSocket` 的路由，再 `serve(opts)`，最后 `injectWebSocket(server)`，顺序不可颠倒。

### 8.4 整体调用顺序小结

1. 解析配置 → `distRoot`、`publicPath`、port、hostname、HTTPS 与证书。
2. 创建 Turbopack project、启动 watch，准备好“HMR 能力”（多连接 Set/WeakMap、broadcast、subscribe/unsubscribe）。
3. `createNodeWebSocket({ app })` 得到 `injectWebSocket`、`upgradeWebSocket`。
4. 在 `app` 上注册：静态（GET/HEAD）、HMR 的 `upgradeWebSocket` 路由、最后 `app.all("*", 405)`。
5. `getPort({ port, host })` 得到 `actualPort`。
6. `serve({ fetch: app.fetch, port: actualPort, hostname, createServer?, serverOptions? })` 得到 `server`。
7. `injectWebSocket(server)`。
8. 注册 SIGINT/SIGTERM：先执行 HMR/Turbopack 清理，再 `server.close()`。

按上述顺序可实现“功能一致、结构清晰”；示例有助于提前暴露“误为先 inject 再 serve”“路由顺序错误导致 405 覆盖静态”等设计问题。

**注意**：实际 HMR 路径与客户端一致，为 **`/turbopack-hmr`**（见 `crates/pack-core/js/src/hmr/client.ts`），示例中的 `/__turbopack-hmr` 需改为 `/turbopack-hmr`。GET 路由即可，Hono 的 `serveStatic` 会同时处理 HEAD。

---

## 九、实施完成与验证

### 9.1 完成状态

- **实现文件**：`packages/pack/src/commands/dev-hono.ts`（与原有 dev 同级）。
- **自测结论**：已按计划完成实现，测试无问题；可与原 dev 行为对齐。
- **入口**：`packages/pack/src/index.ts` 当前仍从 `./commands/dev` 导出 `serve`。验证通过后，可将 `serve` 改为从 `./commands/dev-hono` 导出；原 dev 可重命名为 `dev-legacy.ts` 保留或删除。

### 9.2 实际实现要点（与计划一致或补充）

| 项目 | 说明 |
|------|------|
| **配置解析** | 抽成异步函数 `resolveDevConfig(options, projectPath, rootPath, serverOptions)`，返回 `bundleOptions`、`projectPathResolved`、`rootPathResolved`、`serveOptsBase`（含 port、hostname、可选 createServer/serverOptions）。端口用 `get-port` 得到 `actualPort`。 |
| **ServeOptsBase** | 自定类型（仅 HTTP/HTTPS）：`port`、`hostname` 必选；`createServer`、`serverOptions` 可选。与 hono 的 `ServerType` 联合类型解耦，便于 TS 推断。 |
| **HMR 对接** | `core/hmr.ts` 新增 `WSLike` 接口（`send`、`close`），及 `registerClient` / `unregisterClient` / `handleClientMessage`；`onHMR(req, socket, head)` 标记为 `@deprecated`，供原 dev 使用。dev-hono 直接将 `upgradeWebSocket` 回调中的 `ws`（hono WSContext）传给上述接口，无需再取 raw WebSocket。 |
| **HMR 路径** | 使用 **`/turbopack-hmr`**，与 `crates/pack-core/js/src/hmr/client.ts` 及 next turbopack 一致。 |
| **路由顺序** | 先注册 `GET /turbopack-hmr`（upgradeWebSocket），再 `GET /*`（serveStatic），最后 `app.all("*", 405)`。不单独写 `app.head`，GET 已覆盖 HEAD。 |
| **路径重写** | `publicPath === "runtime"` 或不存在时不剥离前缀（`normalizedPrefix` 置空）；若 `normalizedPrefix` 以 `http://` / `https://` 开头则不重写（与 dev.ts 一致）。 |
| **端口占用** | `actualPort !== port` 时 `console.warn("Port ${port} is in use, using available port ${actualPort} instead.")`。 |
| **进程与信号** | `process.title = "utoopack-dev-server"`；监听 SIGINT/SIGTERM 做 cleanup；并监听 `uncaughtException`、`unhandledRejection`、`rejectionHandled`（与 dev 一致）。 |
| **优雅退出** | cleanup 中先 `hotReloader.close()`，再在存在时调用 `server.closeAllConnections()`（因 ServerType 含 HTTP/2，用运行时判断满足类型），最后 `server.close(callback)`。 |

### 9.3 依赖与类型

- **新增依赖**：`@hono/node-ws`、`get-port`（见 `packages/pack/package.json`）。
- **pack-shared**：`DevServerConfig` 从 config 中抽出并导出，供 dev-hono 解析 devServer 配置；`ServeOptsBase` 仅在 dev-hono 内使用。

### 9.4 后续步骤（维护者操作）

1. 将 `index.ts` 中 `serve` 的导入改为 `./commands/dev-hono`。
2. 视需要将原 `dev.ts` 重命名为 `dev-legacy.ts` 或删除；若保留，可仅作备份或兼容入口。
3. 若不再使用原 dev，可考虑从 pack 中移除 `send` 依赖（需确认无其他引用）。
