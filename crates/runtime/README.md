# utoo-runtime

Node.js 兼容的 JS/TS 运行时，基于 `deno_core`（V8 引擎）构建。

## 快速开始

```bash
# 运行 JS/TS 文件
utoo-runtime run app.ts

# 兼容 node 命令（binary 名为 node 时自动适配）
node app.js
```

## Application Snapshot

Application Snapshot 将应用的完整初始化状态（运行时 + 框架 + 依赖）序列化为 V8 堆快照文件。恢复时跳过 TS 转译、模块解析、框架初始化，直接从快照恢复 V8 堆并重新绑定 TCP 端口，显著降低冷启动时间。

### 使用方式

**1) 构建 Snapshot**

```bash
utoo-runtime snapshot app.ts -o snapshot.bin
```

将执行 `app.ts`，完成框架初始化后将 V8 堆序列化到 `snapshot.bin`。

入口脚本必须调用 `server.listen()`（通过 `http.createServer().listen()` 或 `app.listen()` 等），snapshot 构建阶段会拦截 `listen()` 调用，记录端口和 host 参数但不实际绑定端口。

**2) 从 Snapshot 恢复运行**

```bash
utoo-runtime run app.ts --snapshot snapshot.bin
```

从快照恢复 V8 堆，重新绑定之前记录的端口，开始处理请求。

### 示例

```bash
cd examples/koa-hello
utoo install

# 普通模式运行
utoo-runtime run app.ts
# => Koa listening on http://0.0.0.0:3000

# 构建 snapshot
utoo-runtime snapshot app.ts -o koa-snap.bin
# => Application snapshot written to koa-snap.bin (2325.2 KB)

# 从 snapshot 恢复
utoo-runtime run app.ts --snapshot koa-snap.bin
# 直接就绪，curl http://127.0.0.1:3000/ => Hello from Koa on utoo-runtime!
```

### 工作原理

Snapshot 分两级：

| 级别 | 内容 | 文件 |
|------|------|------|
| Level 1 (Runtime) | Node.js polyfill、内置模块（fs/path/net/...） | `src/snapshot_data.bin`（编译时内置） |
| Level 2 (Application) | 运行时 + 框架 + npm 依赖的完整初始化状态 | 用户通过 `snapshot` 命令生成 |

**Level 2 构建流程**（`build_app_snapshot`）：

1. 创建 `JsRuntimeForSnapshot`，设置 `__utoo_snapshot_mode = true`
2. 替换 `Error.captureStackTrace` 为 fake 实现（避免 V8 内部 CallSite 对象污染快照）
3. 加载并执行入口脚本（TS 自动转译），框架完成初始化
4. `net.Server.listen()` 被拦截：记录 `(server, port, host)` 到 `__utoo_snapshot_servers`，不实际 bind
5. Event loop 自然耗尽（因为没有活跃的 TCP listener）
6. 清理 V8 special properties（`delete Error.stackTraceLimit`，恢复原生 `captureStackTrace`）
7. 调用 `runtime.snapshot()` 序列化 V8 堆

**Level 2 恢复流程**（`run_from_app_snapshot`）：

1. 读取 snapshot 文件，创建 `JsRuntime`（传入 `startup_snapshot`）
2. V8 反序列化堆，恢复所有对象/闭包/模块状态
3. 执行恢复脚本：注册 nextTick 回调，设置 `__utoo_snapshot_mode = false`
4. 遍历 `__utoo_snapshot_servers`，对每个 server 调用真正的 `listen(port, host)`
5. 进入 event loop，开始处理请求

### 已知限制

- 入口脚本必须调用 `server.listen()`，否则 snapshot 构建会报错
- Snapshot 文件与构建时的 `utoo-runtime` 二进制版本绑定，不可跨版本使用
- 依赖 `depd` 的 npm 包（如 `http-errors`、Koa、Express）在 snapshot 模式下 `Error.captureStackTrace` 被替换为 fake 实现，deprecation 警告的 caller 信息不可用
- 尚未支持 `SharedArrayBuffer` 和 `WebAssembly`（Egg.js 等依赖这些的框架暂不可用）

### 性能参考

Koa Hello World（`examples/koa-hello/app.ts`）：

| 模式 | 冷启动 |
|------|--------|
| 普通模式 | ~120ms |
| Snapshot 模式 | ~70ms |
| 提升 | ~40-50% |

框架越重、依赖越多，Snapshot 带来的提升越显著。

## 项目结构

```
src/
├── main.rs          # CLI 入口（run / snapshot 子命令）
├── runtime.rs       # JsRuntime 创建、snapshot 构建/恢复
├── loader.rs        # ESM 模块加载器
├── transpile.rs     # SWC TS/JSX 转译
├── lib.rs           # crate 入口
├── ops/             # Rust op 实现（fs/net/crypto/process/...）
├── js/              # JS polyfill 和 Node.js 内置模块实现
│   ├── bootstrap.js # 全局初始化
│   ├── cjs_loader.js# CommonJS 加载器
│   └── node/        # Node.js 内置模块（fs/path/net/os/...）
├── napi/            # N-API 兼容层
└── snapshot_data.bin# Level 1 运行时快照
```
