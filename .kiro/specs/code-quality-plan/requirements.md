# 需求文档（执行阶段）

## 功能概述

基于已完成的代码质量审计规划（design.md），按优先级与依赖顺序，对 `crates/` 下源码实施全部重构（R-01~R-05）、优化（O-01~O-06）、清理（C-02~C-04）事项。规划阶段（Phase 1）交付物保留为执行依据。

## 用户故事

作为 iota-sympantos 的代码维护者，我希望按照 design.md 中的优先级排序，逐项实施全部重构/优化/清理变更，以便将规划落地为可运行的高质量代码，消除 MVP 验收阻塞项与架构风险。

## 范围说明

**本需求范围内**：
- 实施 design.md §6 重构清单中的 R-01、R-02、R-03、R-05（R-04 审计已完毕，无违规）
- 实施 design.md §7 优化清单中的 O-01 ~ O-06
- 实施 design.md §8 清理清单中的 C-02、C-03、C-04（C-01/C-05/C-06 已完成）
- 修改涉及 `crates/iota-core`、`crates/iota-cli`、`crates/iota-desktop`、`crates/iota-kanban` 源码

**本需求范围外**：
- 不重做 Phase 1 规划（design.md 保持不变作为参考）
- 不修改 `docs/` 目录下文档（C-01/C-05 归档已完成）
- 不引入新 crate 或改变 workspace 结构

## 执行顺序（依赖链）

```
R-02 (proto 抽象) → R-01 (重连/心跳) → O-02 (吞吐基准) + O-05 (集成测试)
R-02 → R-05 (三端统一) → O-01 (token 指标) + O-03 (输出对齐)
C-02 (cache 复核) → R-03 (移除 legacy)
C-03, C-04, O-04, O-06 — 独立可并行
```

## 功能需求

### FR-001: Daemon 协议版本协商抽象（R-02）

- **类型**: 功能需求
- **优先级**: P1（但为 P0 事项的前置依赖，需首先执行）
- **描述**: When 多主题（token-stats、memory-context、daemon-first）共同扩展 daemon proto, the system shall 提供版本协商机制，使新增消息类型不破坏已有客户端。
- **涉及文件**: `crates/iota-core/src/daemon/proto.rs`
- **验收标准**:
  - [ ] AC1.1: WHEN 新消息类型加入 `DaemonClientMessage` / `DaemonServerMessage` THE SYSTEM SHALL 在 `Hello` 握手阶段完成版本协商（含 min/max version 范围）
  - [ ] AC1.2: WHEN 客户端请求的 protocol version 低于 server 支持的最低版本 THE SYSTEM SHALL 返回结构化错误并断开连接
  - [ ] AC1.3: WHEN 版本协商完成 THE SYSTEM SHALL 在连接上下文中记录协商结果，后续消息处理据此决定可用字段
  - [ ] AC1.4: IF 现有 `DESKTOP_PROTOCOL_VERSION = 2` 的行为 THEN 必须向后兼容（version 2 客户端连接新 server 仍正常工作）
  - [ ] AC1.5: WHEN 协议扩展 THE SYSTEM SHALL 保持 `DaemonClientMessage`/`DaemonServerMessage` 为 tagged enum，利用 Rust exhaustive match 保证编译期覆盖

### FR-002: Desktop Daemon 断线重连与心跳（R-01）

- **类型**: 功能需求
- **优先级**: P0（阻塞 MVP 验收）
- **描述**: When desktop daemon TCP 连接意外断开, the system shall 自动重连并恢复会话状态，保证长会话稳定性。
- **涉及文件**: `crates/iota-desktop/src-tauri/src/daemon_client.rs`、`crates/iota-core/src/daemon/proto.rs`
- **前置依赖**: FR-001（版本协商）
- **验收标准**:
  - [ ] AC2.1: WHEN TCP 连接 EOF/error THE SYSTEM SHALL 以指数退避策略自动重连（初始 1s、最大 30s、jitter ±20%）
  - [ ] AC2.2: WHEN 连接空闲超过 30s THE SYSTEM SHALL 发送 Ping 心跳消息，server 回复 Pong
  - [ ] AC2.3: WHEN 连续 3 次心跳无响应 THE SYSTEM SHALL 主动断开并触发重连
  - [ ] AC2.4: WHEN 重连成功 THE SYSTEM SHALL emit 前端事件通知 UI 恢复连接状态（如从「断线」→「已连接」）
  - [ ] AC2.5: IF 重连期间用户发起操作（如 start_turn）THEN 排队等待重连完成后自动重发，不丢弃
  - [ ] AC2.6: WHEN daemon_client 重构完成 THE SYSTEM SHALL 保留现有 600s 超时作为单次操作上限，心跳超时独立于操作超时

### FR-003: 三端 Token 展示数据源统一（R-05）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When CLI / TUI / Desktop 展示 token usage 数据, the system shall 使用统一的 daemon `ObservabilitySummary` 响应作为唯一数据源，消除字段命名漂移。
- **涉及文件**: `crates/iota-cli/src/cli/observability_cmd.rs`、`crates/iota-cli/src/tui/status_bar.rs`、`crates/iota-desktop/src/components/RightInspector.tsx`、`crates/iota-core/src/daemon/proto.rs`
- **前置依赖**: FR-001（版本协商）
- **验收标准**:
  - [ ] AC3.1: WHEN CLI 执行 `tokens` 子命令 THE SYSTEM SHALL 通过 daemon `GetObservabilitySummary` 获取数据（而非直接读 SQLite store）
  - [ ] AC3.2: WHEN TUI 状态栏渲染 token 信息 THE SYSTEM SHALL 消费与 CLI/Desktop 相同的 `ObservabilitySummary` 结构体
  - [ ] AC3.3: WHEN Desktop RightInspector 展示 token_summary THE SYSTEM SHALL 使用与 CLI 相同的字段名和单位
  - [ ] AC3.4: IF CLI 需要离线模式（daemon 未启动）THEN 允许 fallback 直读 store，但 fallback 路径需显式标注且字段名与 daemon 响应一致
  - [ ] AC3.5: WHEN 数据源统一完成 THE SYSTEM SHALL 删除或标记废弃 CLI 中直接读取 `ObservabilityStore` 的旧路径（保留 fallback 除外）

### FR-004: Cache 层 Legacy 迁移代码移除（R-03）

- **类型**: 功能需求
- **优先级**: P2
- **描述**: When 确认 v1 schema 已稳定且线上无 v0 残留数据, the system shall 移除 cache.rs 中的 legacy 迁移路径。
- **涉及文件**: `crates/iota-core/src/store/cache.rs`、`crates/iota-core/src/store/cache_tests.rs`
- **前置依赖**: C-02 人工复核确认
- **验收标准**:
  - [ ] AC4.1: WHEN 迁移代码移除 THE SYSTEM SHALL 删除 `cache.rs:228`（调用点）和 `:281-303`（`migrate_legacy` 函数体 + 临时表 `cache_executions_legacy`）
  - [ ] AC4.2: WHEN 迁移测试移除 THE SYSTEM SHALL 删除 `cache_tests.rs` 中 `migrated_legacy_database_*` 测试用例
  - [ ] AC4.3: WHEN 移除完成 THE SYSTEM SHALL 通过 `cargo test -p iota-core` 全部通过
  - [ ] AC4.4: IF 存在其他代码引用 `migrate_legacy` 或临时表名 THEN 必须一并清理

### FR-005: Token Usage 延迟/吞吐指标暴露（O-01）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When token usage 事件发生, the system shall 记录写入延迟与流式吞吐指标，暴露给 observability 层。
- **涉及文件**: `crates/iota-core/src/store/observability.rs`、`crates/iota-core/src/runtime_event/mod.rs`
- **前置依赖**: FR-003（R-05 三端数据源统一，可选）
- **验收标准**:
  - [ ] AC5.1: WHEN token usage 写入 store THE SYSTEM SHALL 记录写入延迟（p50/p99 histogram）
  - [ ] AC5.2: WHEN daemon 流式推送 token 数据 THE SYSTEM SHALL 记录吞吐量（tokens/s counter）
  - [ ] AC5.3: WHEN observability 查询接口被调用 THE SYSTEM SHALL 在 `ObservabilitySummary` 中包含延迟和吞吐字段
  - [ ] AC5.4: IF 指标注册接口不存在 THEN 需先扩展 observability.rs 添加 histogram/counter 注册能力

### FR-006: Daemon 流式吞吐压测基准（O-02）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When daemon 流式传输 JSON-line 消息, the system shall 提供可复现的压测基准，建立吞吐与延迟的基线数据。
- **涉及文件**: `crates/iota-core/src/daemon/`、新增 bench 文件
- **前置依赖**: FR-002（R-01 重连/心跳）
- **验收标准**:
  - [ ] AC6.1: WHEN 压测执行 THE SYSTEM SHALL 测量 daemon stdout JSON-line 吞吐（msgs/s）
  - [ ] AC6.2: WHEN 压测执行 THE SYSTEM SHALL 测量端到端首 token 延迟（p50/p99 ms）
  - [ ] AC6.3: WHEN 基准建立 THE SYSTEM SHALL 输出可复现的 bench 脚本或 `#[bench]` 测试
  - [ ] AC6.4: IF 吞吐低于 100 msgs/s THEN 标记为性能瓶颈需优化

### FR-007: 三端 Observability 输出对齐（O-03）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When CLI / TUI / Desktop 渲染 observability 数据, the system shall 保证同一 daemon 响应在三端的字段名、单位、舍入规则完全一致。
- **涉及文件**: `crates/iota-cli/src/cli/observability_cmd.rs`、`crates/iota-cli/src/tui/status_bar.rs`、`crates/iota-desktop/src/components/RightInspector.tsx`
- **前置依赖**: FR-003（R-05 数据源统一）
- **验收标准**:
  - [ ] AC7.1: WHEN 三端渲染 token count THE SYSTEM SHALL 使用相同字段名（如统一为 `total_tokens`、`input_tokens`、`output_tokens`）
  - [ ] AC7.2: WHEN 三端渲染 cost THE SYSTEM SHALL 使用相同单位（USD）和精度（小数点后 4 位）
  - [ ] AC7.3: WHEN 三端渲染 duration THE SYSTEM SHALL 使用相同单位（ms）和舍入规则（round half up）
  - [ ] AC7.4: IF 新增字段到 `ObservabilitySummary` THEN 三端必须同步更新渲染逻辑

### FR-008: 单元测试覆盖缺口扫描与补齐（O-04）

- **类型**: 功能需求
- **优先级**: P2
- **描述**: When 测试覆盖度不足, the system shall 识别 `crates/iota-core/src/store/`、`acp/`、`memory/`、`context/` 模块中缺少对应 `*_tests.rs` 的文件，并补充关键测试。
- **涉及文件**: `crates/iota-core/src/` 各子模块
- **前置依赖**: 无
- **验收标准**:
  - [ ] AC8.1: WHEN 扫描完成 THE SYSTEM SHALL 列出所有缺少 `*_tests.rs` 对应文件的源模块
  - [ ] AC8.2: WHEN 缺口补齐 THE SYSTEM SHALL 为每个核心公开函数至少编写 1 个正向测试
  - [ ] AC8.3: WHEN 测试编写 THE SYSTEM SHALL 遵循 ut-standardization 约定（独立 `*_tests.rs` 文件，禁止内联 `mod tests`）
  - [ ] AC8.4: IF 某模块无法单独测试（依赖外部 daemon/网络）THEN 使用 mock trait 或标注 `#[ignore]`

### FR-009: 集成/验收测试补齐（O-05）

- **类型**: 功能需求
- **优先级**: P0（阻塞 MVP 验收）
- **描述**: When MVP 验收场景缺少自动化测试, the system shall 为 daemon ↔ ACP 端到端流程与 desktop-mvp-acceptance.md 中的关键场景编写集成测试。
- **涉及文件**: `crates/iota-core/src/daemon/`、`crates/iota-desktop/src-tauri/`
- **前置依赖**: FR-002（R-01 重连/心跳）
- **验收标准**:
  - [ ] AC9.1: WHEN daemon 启动 THE SYSTEM SHALL 有集成测试验证 client connect → Hello 握手 → StartTurn → 响应流 完整链路
  - [ ] AC9.2: WHEN 连接断开 THE SYSTEM SHALL 有测试验证自动重连恢复（FR-002 AC2.1 的自动化验证）
  - [ ] AC9.3: WHEN Kanban dispatcher 事件同步 THE SYSTEM SHALL 有集成测试覆盖 bridge → event_sync → desktop 通知链路
  - [ ] AC9.4: IF desktop-mvp-acceptance.md 中的场景无法自动化 THEN 编写手工测试 runbook 并标注原因

### FR-010: MCP 跨语言 SDK 漂移检测（O-06）

- **类型**: 功能需求
- **优先级**: P2
- **描述**: When MCP 协议在 Rust core 与 `iota-fun` 7 语言 sidecar 之间可能存在字段漂移, the system shall 建立首次对比基线并输出漂移报告。
- **涉及文件**: `crates/iota-core/src/mcp/`（`client.rs`、`server.rs`、`router.rs`、`tool_dispatch.rs`）
- **前置依赖**: 无
- **验收标准**:
  - [ ] AC10.1: WHEN 基线建立 THE SYSTEM SHALL 提取 Rust MCP 模块的协议字段清单（消息类型、字段名、类型）
  - [ ] AC10.2: WHEN 对比执行 THE SYSTEM SHALL 输出漂移表（Rust 有/sidecar 无、sidecar 有/Rust 无）
  - [ ] AC10.3: IF 漂移项影响功能正确性 THEN 标记为 P1 修复项
  - [ ] AC10.4: IF `iota-fun` 仓库不可访问 THEN 仅输出 Rust 侧基线，标注「sidecar 对比待补充」

### FR-011: Cache Legacy 迁移代码人工复核（C-02）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When cache.rs legacy 迁移路径需要确认可安全移除, the system shall 验证线上是否仍存在 v0 schema 数据。
- **涉及文件**: `crates/iota-core/src/store/cache.rs:228,281-303`
- **前置依赖**: 无
- **验收标准**:
  - [ ] AC11.1: WHEN 复核执行 THE SYSTEM SHALL 检查迁移函数是否为幂等（可重复执行无副作用）
  - [ ] AC11.2: WHEN 复核执行 THE SYSTEM SHALL 确认 `cache_executions_legacy` 临时表在正常流程中不会被创建（仅迁移路径触发）
  - [ ] AC11.3: IF 迁移为幂等且无外部依赖 THEN 标记为可安全移除（解锁 FR-004）
  - [ ] AC11.4: IF 发现线上可能仍有 v0 数据 THEN 保留迁移代码并标注等待窗口

### FR-012: Approvals Fallback 注释优化（C-03）+ Desktop 文件审查（C-04）

- **类型**: 功能需求
- **优先级**: P1
- **描述**: When 代码中存在误导性命名（「legacy fallback」实为安全兜底）或未经 lint 验证的文件, the system shall 优化注释/命名并在可能时运行 clippy/tsc 检查。
- **涉及文件**: `crates/iota-core/src/store/approvals.rs:286`、`crates/iota-desktop/` 相关组件
- **前置依赖**: 无
- **验收标准**:
  - [ ] AC12.1: WHEN approvals.rs:286 注释优化 THE SYSTEM SHALL 将 `Fallback legacy blacklists` 改为更准确的描述（如 `Defense-in-depth path traversal check`）
  - [ ] AC12.2: WHEN Desktop 文件审查 THE SYSTEM SHALL 对 `daemon_client.rs`、`lib.rs`、`api.ts`、核心组件运行 `cargo clippy`（Rust）和 `tsc --noEmit`（TypeScript）
  - [ ] AC12.3: IF clippy/tsc 报告 warning THE SYSTEM SHALL 修复所有 warning（不允许 `#[allow]` 压制除非有充分理由）
  - [ ] AC12.4: IF 环境不支持 clippy/tsc THEN 进行人工代码审查并记录发现

## 技术约束

- **依赖规则**（不可违反）：`iota-core/src/acp/` 不得依赖 CLI/TUI/desktop/daemon UI 层；store 模块只暴露 typed operations；presentation 层不得直接拥有 ACP session
- **测试约定**：测试必须位于独立 `*_tests.rs` 文件中，禁止内联 `mod tests`（ut-standardization 约定）
- **配置唯一源**：所有配置只从 `~/.i6/nimia.yaml` 读取，不引入新的配置入口
- **向后兼容**：daemon 协议变更必须兼容 `DESKTOP_PROTOCOL_VERSION = 2` 的已有客户端
- **代码现状权威源**：当文档与代码冲突时，以代码为准
- **不引入新依赖**：除非实现必须（如心跳需要 tokio timer），不新增 crate 依赖

## API 依赖

本需求修改的内部协议接口：

| 协议 | 当前位置 | 变更类型 |
|------|---------|---------|
| Daemon 协议 | `crates/iota-core/src/daemon/proto.rs` | 扩展：新增 Ping/Pong 消息、版本协商字段 |
| ObservabilitySummary | `crates/iota-core/src/store/observability.rs` | 扩展：新增延迟/吞吐指标字段 |
| Desktop ↔ Daemon 连接 | `crates/iota-desktop/src-tauri/src/daemon_client.rs` | 重构：加入重连/心跳状态机 |

## 非功能需求

### NFR-001: 编译兼容性

- **类型**: 非功能需求 - 兼容性
- **AC**:
  - [ ] AC1.1: 每个 FR 完成后 `cargo build --workspace` 无 error
  - [ ] AC1.2: 每个 FR 完成后 `cargo test --workspace` 无新增 failure
  - [ ] AC1.3: TypeScript 变更后 `tsc --noEmit` 无 error（如环境支持）

### NFR-002: 增量可交付

- **类型**: 非功能需求 - 可维护性
- **AC**:
  - [ ] AC2.1: 每个 FR 作为独立 commit，commit message 包含对应 FR-ID
  - [ ] AC2.2: FR 之间的依赖关系严格按执行顺序，不允许跳过前置依赖
  - [ ] AC2.3: 任何 FR 中断后，已完成的 FR 代码仍可独立编译运行

### NFR-003: 性能无退化

- **类型**: 非功能需求 - 性能
- **AC**:
  - [ ] AC3.1: 心跳/重连机制不增加正常通信路径的延迟（Ping/Pong 仅在空闲时触发）
  - [ ] AC3.2: observability 指标记录不阻塞主业务路径（异步写入或 fire-and-forget）
