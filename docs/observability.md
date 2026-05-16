# Observability

本文只记录当前实现，避免保留旧的本地 event 聚合设计。

## 入口

| 命令 | 数据源 | 用途 |
|---|---|---|
| `iota run --timing ...` | `AcpPromptTiming` | 输出 route、spawn、init、prompt、total 耗时 |
| `iota run --log-events ...` | `AcpPromptOutput.events` | 输出本轮 normalized `RuntimeEvent` |
| `iota logs <execution_id>` | Loki HTTP API | 按 `iota_execution_id` 查询远端日志 |
| `iota trace <trace_id>` | Jaeger HTTP API | 查询 trace span 并打印简要瀑布 |

环境变量：

- `IOTA_LOKI_URL`，默认 `http://localhost:3100`
- `IOTA_JAEGER_URL`，默认 `http://localhost:16686`

## Runtime Events

ACP update、complete、permission、usage、tool 和 error 会被归一化为 `RuntimeEvent`，随 `AcpPromptOutput.events` 返回。CLI 的 `--log-events` 直接打印这些事件；TUI 用它更新历史、approval 和流式输出状态。

## Metrics

`telemetry::metrics` 使用 OpenTelemetry meter 注册当前进程内指标：

| 指标 | 含义 |
|---|---|
| `iota.execution.count` | execution 结束计数，按 status 记录 |
| `iota.cache.hit.count` / `iota.cache.miss.count` | CacheStore replay/join 命中情况 |
| `iota.prompt.queued` | TUI prompt 队列长度变化 |
| `iota.token.*` | token usage 事件和 input/output/total token |
| `iota.prompt.duration` / `iota.init.duration` | prompt 与 ACP init 耗时直方图 |

如需导出 OTLP，使用 `telemetry::init_otel()` 配置 trace、metric、log exporter；默认 CLI 路径仍可只使用 stderr/file tracing。

## Execution Cache

`CacheStore` 位于 `store/cache.rs`，当前只负责 execution replay / dedupe：

```text
run_prompt_with_optional_execution_id()
  -> request_hash()
  -> find_completed_by_request_hash()  # replay
  -> find_running_by_request_hash()    # join in-flight
  -> begin_execution_with_id()         # lock + fencing token
  -> append_output(OutputEvent)        # only output events for replay
  -> finish_execution()
```

它使用 `~/.i6/context/events.sqlite` 的 cache tables，但不再承载完整 observability 聚合、Prometheus 输出或任意 event 查询。

## 日志边界

- 工程日志：`tracing` / file appender / stderr layer，用于排查程序自身行为。
- 运行事件：`RuntimeEvent`，用于 CLI/TUI 展示和部分 replay。
- 外部观测：Loki/Jaeger 查询命令只读取外部服务，不依赖本地 SQLite 聚合。
