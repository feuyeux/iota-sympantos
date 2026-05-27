# Observability

本文记录当前实现：RuntimeEvent、token usage 持久化、`iota observability` 本地查询、OpenTelemetry、Loki 和 Jaeger 边界。

## 命令入口

| 命令 | 数据源 | 用途 |
| :--- | :--- | :--- |
| `iota run --timing ...` | `AcpPromptTiming` | 输出 route、spawn、init、session、prompt、total 耗时 |
| `iota run --log-events ...` | `AcpPromptOutput.events` | 输出本轮 normalized `RuntimeEvent` |
| `iota observability logging recent --limit N` | `ObservabilityStore` | 最近 execution-level token 记录 |
| `iota observability logging events <execution_id>` | `ObservabilityStore` | 单个 execution 的 raw token usage events |
| `iota observability tokens recent --limit N [--json]` | `ObservabilityStore` | 最近 token usage 明细 |
| `iota observability tokens summary --since 1h [--json]` | `ObservabilityStore` | backend 维度 token 均值、标准差、CV 和计数 |
| `iota observability tokens export --format json` | `ObservabilityStore` | 导出 token usage 明细 |
| `iota observability metrics --prometheus` | `ObservabilityStore` | 输出 Prometheus 文本格式的本地 token 聚合指标 |
| `iota observability logs <execution_id>` | Loki HTTP API | 按 `iota_execution_id` 查询远端日志 |
| `iota observability trace <trace_id>` | Jaeger HTTP API | 查询 trace span 并打印简要瀑布 |
| `iota logs <execution_id>` | Loki HTTP API | 顶层别名 |
| `iota trace <trace_id>` | Jaeger HTTP API | 顶层别名 |

环境变量：

- `IOTA_LOKI_URL`，默认 `http://localhost:3100`
- `IOTA_JAEGER_URL`，默认 `http://localhost:16686`
- `OTEL_ENABLED=true` 启用 OTLP 导出
- `OTEL_EXPORTER_OTLP_ENDPOINT`，默认 collector endpoint 为 `http://localhost:4317`

## RuntimeEvent

ACP update、complete、permission、usage、tool 和 error 会被归一化为 `RuntimeEvent`，随 `AcpPromptOutput.events` 返回。CLI 用 `--log-events` 打印；TUI 和 desktop 用它更新 transcript、approval、token breakdown、tool call 和 inspector 状态。

主要事件：

```text
Output
State
Log
ToolCall
ToolResult
Error
Extension
TokenUsage
Memory
ApprovalRequest
ApprovalDecision
```

## Token Usage

`RuntimeEvent::TokenUsage` 统一承载 OpenAI、Anthropic、Gemini 和 adapter-only usage 字段。

| 字段 | 含义 |
| :--- | :--- |
| `input_tokens` / `output_tokens` | 输入和输出 token |
| `cache_read_input_tokens` | 缓存命中的输入 token |
| `cache_creation_input_tokens` | 缓存写入的输入 token |
| `thinking_tokens` | reasoning / thoughts / thinking token |
| `tool_use_prompt_tokens` | 工具结果回灌 token，provider 支持时填充 |
| `provider_reported_total_tokens` | provider 或 adapter 原样上报 total |
| `normalized_total_tokens` | iota 归一化后的 total，字段不足时为 `None` |
| `raw_payload` | 原始 usage JSON |

`ObservabilityStore` 使用 `~/.i6/context/events.sqlite`。同一 execution 中如果同时存在 streaming `usage_update` 和 final `usage`，查询层优先选择字段更完整的 final usage。字段缺失不按 0 计入 summary。

## Metrics

`telemetry::metrics` 注册进程内 OpenTelemetry meter：

| 指标 | 含义 |
| :--- | :--- |
| `iota.execution.count` | execution 结束计数，按 status 记录 |
| `iota.prompt.queued` | TUI prompt 队列长度变化 |
| `iota.token.*` | token usage 事件和 input/cache/output/thinking/total token |
| `iota.prompt.duration` / `iota.init.duration` | prompt 与 ACP init 耗时直方图 |

默认 CLI 只初始化本地 tracing，不要求 collector。Docker compose 会提供 OpenTelemetry Collector、Jaeger、Prometheus、Loki 和 Grafana。

## 日志边界

- 工程日志：`tracing`、file appender、stderr layer，用于排查程序自身行为。
- 运行事件：`RuntimeEvent`，用于 CLI/TUI/desktop 展示，并作为 token usage 落库输入。
- 本地观测：`iota observability` 读取 SQLite store，聚焦 token usage 和 Prometheus 文本指标。
- 外部观测：Loki/Jaeger 查询命令只读取外部服务，不依赖本地 SQLite 聚合。
