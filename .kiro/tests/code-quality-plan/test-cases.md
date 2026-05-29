# 测试用例 — code-quality-plan（执行阶段）

> **测试对象类型**：代码变更实施（R-01~R-05 / O-01~O-06 / C-02~C-04），涉及 `crates/` 下 Rust + TypeScript 源码
> **测试方法**：shell 命令验证（grep / 文件读取 / 结构校验），test_method: `shell`
> 由于 Pod 环境无 Rust 工具链（`cargo` 不可用），无法执行编译/测试命令，改用代码结构 + grep 验证。

## 用例总览

| 用例号 | 描述 | 关联需求 | 测试方法 |
|--------|------|---------|---------|
| TC-01 | 协议版本协商实现完整性 | FR-001 (AC1.1~1.5) | 代码结构 + grep |
| TC-02 | 断线重连与心跳机制 | FR-002 (AC2.1~2.6) | 代码结构 + 逻辑验证 |
| TC-03 | 三端 Token 数据源统一 | FR-003 (AC3.1~3.5) | 代码路径验证 |
| TC-04 | Cache Legacy 迁移代码移除 | FR-004 (AC4.1~4.4), FR-011 (AC11.1~11.4) | grep 验证 |
| TC-05 | Token 延迟/吞吐指标暴露 | FR-005 (AC5.1~5.4) | 代码结构验证 |
| TC-06 | Daemon 流式吞吐压测基准 | FR-006 (AC6.1~6.4) | bench 文件存在性 + 内容 |
| TC-07 | 三端输出对齐 + MCP 基线 + 代码清理 | FR-007, FR-010, FR-012 | 字段对齐 + grep |
| TC-08 | 测试补齐（单元 + 集成） | FR-008, FR-009 | 文件存在性 + 内容 |

---

## TC-01：协议版本协商实现完整性

**步骤**
1. 确认 `proto.rs` 存在 `PROTOCOL_VERSION_MIN`/`MAX` 常量
2. 确认 `DaemonClientMessage::Hello` 含 `min_version`/`max_version` 字段（`serde(default)`）
3. 确认 `DaemonServerMessage::HelloAccepted` 含 `negotiated_version` 字段
4. 确认 `desktop.rs` 存在 `negotiate_version()` 函数及 `DesktopConnectionContext`
5. 确认 `daemon_client.rs` 的 `hello_message()` 发送 min/max version
6. 确认 `proto_tests.rs` 含版本协商成功/失败/v2兼容测试
7. 确认 `DaemonClientMessage`/`DaemonServerMessage` 为 tagged enum（`#[serde(tag = "type")]`）

**期望**：FR-001 AC1.1~AC1.5 全部满足。

## TC-02：断线重连与心跳机制

**步骤**
1. 确认 `daemon_client.rs` 含 `ReconnectConfig`（initial=1000ms, max=30000ms, jitter=20%）
2. 确认 `reconnect_with_backoff()` 实现指数退避 + jitter
3. 确认 `start_heartbeat_loop()` 以 30s 间隔发 Ping
4. 确认心跳仅 `negotiated_version >= 3` 启用
5. 确认连续 3 次无 Pong 触发断开重连
6. 确认 `types.ts` 含 `DaemonConnectionState` 类型
7. 确认 emit `"daemon-connection-state"` 事件
8. 确认 `pending_queue` / `send_or_queue` 实现操作排队
9. 确认 `timeout_ms: Some(600_000)` 保留 600s 操作超时

**期望**：FR-002 AC2.1~AC2.6 全部满足。

## TC-03：三端 Token 数据源统一

**步骤**
1. 确认 `observability_cmd.rs` 有 daemon 优先路径 `try_daemon_observability_summary()`
2. 确认有 offline fallback（直读 store）且有注释标注
3. 确认 `ObservabilitySummaryResponse` 为强类型结构体（非 `serde_json::Value`）
4. 确认 `TokenSummaryEntry` 含 `input_tokens_mean`/`output_tokens_mean`/`normalized_total_mean`
5. 确认 `RecentTokenExecution` 含 `input_tokens`/`output_tokens`/`normalized_total_tokens`
6. 确认 `types.ts` `ObservabilitySummary` 字段名与 Rust 端一致

**期望**：FR-003 AC3.1~AC3.5, FR-007 AC7.1~AC7.4 全部满足。

## TC-04：Cache Legacy 迁移代码移除

**步骤**
1. grep `migrate_legacy` 在 `crates/` 下为 0 匹配
2. grep `cache_executions_legacy` 为 0 匹配
3. 确认 `cache_tests.rs` 无 `migrated_legacy_database` 测试
4. 确认 `cache.rs` init() 仅创建 `cache_executions` 表 + index

**期望**：FR-004 AC4.1~AC4.4, FR-011 AC11.1~AC11.4 全部满足。

## TC-05：Token 延迟/吞吐指标暴露

**步骤**
1. 确认 `observability.rs` 含 `ObservabilityMetrics` 结构体
2. 确认 `record_write_latency()` / `record_stream_throughput()` 方法存在
3. 确认 `write_latency_percentiles()` 返回 `LatencyPercentiles { p50_ms, p99_ms, count }`
4. 确认 `stream_throughput_summary()` 返回 `ThroughputSummary { mean_tokens_per_sec, count }`
5. 确认 `ObservabilitySummaryResponse` 含 `write_latency`/`stream_throughput` 字段
6. 确认 `record_token_usage_with_metrics()` 测量写入延迟
7. 确认 `observability_tests.rs` 含指标相关测试

**期望**：FR-005 AC5.1~AC5.4 全部满足。

## TC-06：Daemon 流式吞吐压测基准

**步骤**
1. 确认 `crates/iota-core/benches/daemon_throughput.rs` 存在
2. 确认含 `bench_jsonline_throughput` 函数（测量 msgs/s）
3. 确认含 `bench_first_token_latency` 函数
4. 确认 `Cargo.toml` 含 criterion dev-dependency + `[[bench]]` target
5. 确认 bench 函数覆盖序列化和反序列化

**期望**：FR-006 AC6.1~AC6.4 全部满足。

## TC-07：三端输出对齐 + MCP 基线 + 代码清理

**步骤**
1. 确认 `approvals.rs:286` 注释为 `Defense-in-depth: path traversal check`（非 `Fallback legacy blacklists`）
2. grep `Fallback legacy` 为 0 匹配
3. 确认 `mcp-baseline.md` 存在且含协议字段清单
4. 确认 MCP baseline 标注 sidecar 对比「待补充」（iota-fun 不可访问时）
5. 确认 `types.ts` `DaemonServerMessage` 含 `pong` 变体

**期望**：FR-007, FR-010, FR-012 全部满足。

## TC-08：测试补齐（单元 + 集成）

**步骤**
1. 确认 `crates/iota-core/src/mcp/client_tests.rs` 存在
2. 确认 `client.rs` 含 `#[cfg(test)]` + `mod tests` 声明
3. 确认 `crates/iota-core/tests/daemon_integration.rs` 存在
4. 确认集成测试含 Hello 握手、Ping/Pong、StartTurn roundtrip
5. 确认含 `#[ignore]` 标注的完整链路测试（需 daemon）
6. 确认含手工 runbook（Kanban dispatcher 场景）

**期望**：FR-008 AC8.1~AC8.4, FR-009 AC9.1~AC9.4 全部满足。
