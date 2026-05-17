# Observability

本文只记录当前实现，覆盖 RuntimeEvent、token usage 持久化、`iota observability` 本地查询、外部 logs/trace 和 metrics 边界。

## 入口

| 命令 | 数据源 | 用途 |
|---|---|---|
| `iota run --timing ...` | `AcpPromptTiming` | 输出 route、spawn、init、prompt、total 耗时 |
| `iota run --log-events ...` | `AcpPromptOutput.events` | 输出本轮 normalized `RuntimeEvent` |
| `iota observability logging recent --limit N` | `ObservabilityStore` | 输出最近 execution-level token 记录 |
| `iota observability logging events <execution_id>` | `ObservabilityStore` | 输出某个 execution 的 raw token usage events |
| `iota observability tokens recent --limit N [--json]` | `ObservabilityStore` | 输出最近 token usage 明细 |
| `iota observability tokens summary --since 1h [--json]` | `ObservabilityStore` | 按 backend 输出 token 均值、标准差和计数 |
| `iota observability tokens export --format json` | `ObservabilityStore` | 导出 token usage 明细 JSON |
| `iota observability metrics --prometheus` | `ObservabilityStore` | 输出本地 token 聚合指标 |
| `iota logs <execution_id>` | Loki HTTP API | 按 `iota_execution_id` 查询远端日志 |
| `iota trace <trace_id>` | Jaeger HTTP API | 查询 trace span 并打印简要瀑布 |

环境变量：

- `IOTA_LOKI_URL`，默认 `http://localhost:3100`
- `IOTA_JAEGER_URL`，默认 `http://localhost:16686`

## Runtime Events

ACP update、complete、permission、usage、tool 和 error 会被归一化为 `RuntimeEvent`，随 `AcpPromptOutput.events` 返回。CLI 的 `--log-events` 直接打印这些事件；TUI 用它更新历史、approval、token breakdown 和流式输出状态。

### TokenUsage

`RuntimeEvent::TokenUsage` 统一承载 OpenAI、Anthropic、Gemini 和 adapter-only usage 字段。核心字段包括：

| 字段 | 含义 |
|---|---|
| `input_tokens` / `output_tokens` | 输入和输出 token |
| `cache_read_input_tokens` | 缓存命中的输入 token |
| `cache_creation_input_tokens` | 缓存写入的输入 token |
| `thinking_tokens` | reasoning / thoughts / thinking token |
| `tool_use_prompt_tokens` | 工具结果回灌 token，provider 支持时填充 |
| `provider_reported_total_tokens` | provider 或 adapter 原样上报 total |
| `normalized_total_tokens` | iota 归一化后的 total，字段不足时为 `None` |
| `raw_payload` | 原始 usage JSON，用于后续回溯 |

Gemini 的 `promptTokenCount` 已包含 cached content token；Anthropic 的完整输入口径需要 `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`。Codex ACP 当前可能只提供 `usage_update.used`，因此只进入 `provider_reported_total_tokens`。

## Metrics

`telemetry::metrics` 使用 OpenTelemetry meter 注册当前进程内指标：

| 指标 | 含义 |
|---|---|
| `iota.execution.count` | execution 结束计数，按 status 记录 |
| `iota.prompt.queued` | TUI prompt 队列长度变化 |
| `iota.token.*` | token usage 事件和 input/cache/output/thinking/total token |
| `iota.prompt.duration` / `iota.init.duration` | prompt 与 ACP init 耗时直方图 |

如需导出 OTLP，使用 `telemetry::init_otel()` 配置 trace、metric、log exporter；默认 CLI 路径仍可只使用 stderr/file tracing。

## Local Stores

`CacheStore` 位于 `store/cache.rs`，当前只负责 execution lifecycle：

```text
run()
  -> request_hash()
  -> begin_execution_with_id()         # execution id + fencing token
  -> finish_execution()
```

它使用 `~/.i6/context/events.sqlite` 的 cache tables。

`ObservabilityStore` 位于 `store/observability.rs`，同样使用 `~/.i6/context/events.sqlite`，负责 token usage 持久化和查询：

```text
RuntimeEvent::TokenUsage
  -> engine::telemetry::record_runtime_event()
  -> ObservabilityStore::record_token_usage()
  -> token_usage_events
```

`tokens recent`、`tokens summary` 和 `metrics --prometheus` 使用 execution-level 去重视图：同一 execution 中如果同时存在 `usage_update` 和 final `usage`，优先选择字段更完整的 final usage。

## 日志边界

- 工程日志：`tracing` / file appender / stderr layer，用于排查程序自身行为。
- 运行事件：`RuntimeEvent`，用于 CLI/TUI 展示，并作为 token usage 落库输入。
- 本地观测：`iota observability` 读取 `ObservabilityStore`，当前聚焦 token usage 明细、汇总和 metrics。
- 外部观测：Loki/Jaeger 查询命令只读取外部服务，不依赖本地 SQLite 聚合。
