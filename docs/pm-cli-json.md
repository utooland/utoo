# PM CLI JSON contract / PM CLI JSON 契约

`utoo --json` emits one compact JSON document. Success is written to stdout
with exit code `0`; failure is written to stderr with the documented non-zero
exit code. Progress, lifecycle output, warnings, and update notices are not
written outside that document.

`utoo --json` 只输出一个紧凑 JSON 文档。成功时写入 stdout，退出码为 `0`；
失败时写入 stderr，并使用约定的非零退出码。进度、生命周期输出、警告和更新
提示不会泄漏到文档之外。

The machine-readable source of truth is
[`crates/pm/schema/cli-output-v1.schema.json`](../crates/pm/schema/cli-output-v1.schema.json).

## Envelope / 顶层结构

Success:

```json
{"schemaVersion":1,"command":"install","ok":true,"durationMs":42,"result":{}}
```

Failure:

```json
{"schemaVersion":1,"command":"install","ok":false,"durationMs":42,"error":{"category":"local","code":"operation_failed","exitCode":11,"message":"..."}}
```

- `command` is the canonical first-level command; aliases are normalized. /
  `command` 是规范化的一级命令，别名会归一化。
- `subcommand` is present for commands such as `config get` and `config delete` (`rm`). /
  `config get`、`config delete`（`rm`）等命令使用 `subcommand`。
- `command` may be `null` only when argument parsing cannot identify it. /
  仅当参数解析无法识别命令时，`command` 才可能为 `null`。
- `result` and `error` are mutually exclusive. / `result` 与 `error` 互斥。
- `durationMs` covers the full invocation using a monotonic clock. /
  `durationMs` 使用单调时钟统计完整调用耗时。

## Result families / 命令结果

| `command` | Result |
| --- | --- |
| `install`, `uninstall`, `update`, `rebuild` | Dependency operation and summary / 依赖操作与摘要 |
| `clean`, `deps`, `list`, `link` | Filesystem or dependency result / 文件与依赖结果 |
| `run` | Ordered lifecycle executions and skipped workspaces / 有序生命周期执行结果 |
| `execute`, `custom` | Captured process execution / 捕获的进程执行结果 |
| `view`, `pack`, `publish` | Registry or package artifact metadata / Registry 与包产物元数据 |
| `ping`, `whoami`, `logout` | Registry/authentication result / Registry 与认证结果 |
| `config` | KV values plus `set`, `get`, `delete` (`rm`), or `list` / KV 值及 `set`、`get`、`delete`（`rm`）、`list` 子命令 |
| `init`, `completions`, `help`, `version` | Generated artifact or CLI metadata / 生成产物或 CLI 元数据 |

`login --json` returns the usage error `interactive_required`; the browser login
flow remains human-only. `login --json` 返回 `interactive_required` usage
错误，浏览器登录流程仍仅支持 human 模式。

## Errors / 错误

| Category | Exit code | Recovery meaning |
| --- | ---: | --- |
| `transient` | 1 | Retry may succeed / 可重试 |
| `usage` | 2 | Change arguments or mode / 修改参数或调用模式 |
| `auth` | 3 | Refresh credentials / 更新认证信息 |
| `not_found` | 4 | Resource is absent / 资源不存在 |
| `rate_limited` | 5 | Retry after throttling / 限流后重试 |
| `precondition` | 7 | Satisfy required state / 先满足前置条件 |
| `local` | 11 | Fix local state / 修复本地进程、文件或项目状态 |

`error.code` is a non-empty producer-defined identifier. Consumers should use
`category` as the fallback and must not match `message`, `causes`, or
`suggestion`. Optional `details` is a typed union with `lifecycle`,
`dependency`, `registry`, `filesystem`, or `process` kind. Optional
`partialResult` contains only completed irreversible work.

消费者应优先判断 `error.code`，未知 code 回退到 `category`，不要匹配
`message`、`causes` 或 `suggestion`。`details` 是带 `kind` 的类型化联合；
`partialResult` 只记录已完成且不可逆的操作。

Captured stdout and stderr use:

```json
{"tail":"...","bytes":70000,"truncated":true}
```

Each stream retains its final 64 KiB. Paths to real files use absolute native
platform syntax; package-internal paths are relative and use `/`.
每个流保留最后 64 KiB；真实文件路径使用平台原生绝对路径，包内路径使用
`/` 分隔的相对路径。

JSON output is not automatically redacted and can contain commands, paths, and
captured process output. Treat it with the same sensitivity as terminal logs.
JSON 不自动脱敏，可能包含命令、路径和进程输出，应按终端日志同等保护。

## Compatibility / 兼容性

Consumers must ignore unknown object fields. Adding optional fields is
compatible within `schemaVersion: 1`; removing or renaming fields, changing
their type, or changing their meaning requires a new major schema version.
Collections and maps have deterministic ordering; execution results preserve
topological order, with same-layer workspaces ordered by canonical name.

消费者必须忽略未知对象字段。版本 1 内允许新增可选字段；删除、重命名、改
类型或改语义必须升级主 schema 版本。集合顺序稳定；执行结果保持拓扑顺序，
同层 workspace 按规范名称排序。

Regenerate the checked-in schema with:

```console
UPDATE=1 cargo test -p utoo-pm cli_output_schema
```
