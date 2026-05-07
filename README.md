# iota sympantos

Cross-platform Rust CLI/TUI，将 prompt 路由到五个 ACP 后端（claude-code / codex / gemini / hermes / opencode），共享统一的记忆、技能与上下文层。

## 核心功能

- **跨后端记忆** — Rust 引擎层 SQLite 存储（SHA-256 去重、FTS5、6 召回桶）。任一后端写入的记忆可在其他后端召回注入。
- **确定性技能** — YAML 声明的技能由 Rust 引擎分发，触发匹配与输出模板与后端无关，所有后端产出一致的结构化结果。
- **iota-fun 多语言执行** — 7 语言片段运行器（C++ / TypeScript / Rust / Zig / Java / Python / Go），含编译缓存与 `parallel: true` 支持。
- **Daemon 热路径** — 可选 TCP daemon 保持 ACP 客户端预热；任何命令加 `--daemon/-d` 即可路由。
- **交互式 TUI** — ratatui 循环，含多行编辑器、Markdown 渲染、流式输出与权限审批覆层。

## 架构

![Architecture Overview](images/iota-sympantos-architecture.png)

| 层级 | 模块 |
|------|------|
| **UI** | `cli.rs`, `tui.rs` + `tui/` |
| **编排** | `engine.rs`, `agent.rs`, `acp*.rs`, `mcp_*.rs`, `context.rs`, `skills.rs`, `skill_runner.rs` |
| **存储** | `memory.rs`, `event_store.rs`, `session_ledger.rs`, `approval.rs` |

详见 [`doc/architecture.md`](doc/architecture.md) 和 [`doc/code-call-chains.md`](doc/code-call-chains.md)。

## 功能实验室

| # | 主题 | 报告 |
|---|------|------|
| 01 | 跨后端记忆延续 — 6 召回桶、SHA-256 去重、置信度过滤、token 预算 | [`gefsi/exp01-memory.md`](gefsi/exp01-memory.md) |
| 02 | Skill + iota-fun 多语言执行 — 触发匹配、并行工具、编译缓存、5 后端一致性 | [`gefsi/exp02-skill-fun.md`](gefsi/exp02-skill-fun.md) |

## 快速开始

### 构建

```bash
cargo build --offline
cargo install --path .
```

### 配置

配置文件：`~/.i6/nimia.yaml`，每个后端的关键字段：

```yaml
codex:
  enabled: true
  acp:
    command: npx
    args: ["-y", "@zed-industries/codex-acp@0.12.0"]
  version_mapping:
    acp: "0.12.0"
    bin: "0.128.0"
  model:
    provider: ninerouter
    name: gh/gpt-5.5
    base_url: http://localhost:20128/v1
    api_key: "<router-api-key>"
```

`iota check` 查看所有后端的生效配置。

### 运行

```bash
iota                                              # 交互式 TUI
iota run codex "ping"                             # 单次 prompt，直连
iota run --daemon codex --timeout-ms 20000 "ping" # 经由 daemon（热路径）
iota check                                        # 检查配置与后端状态
```

`--trace-timing` 将路由与 ACP 阶段耗时以 JSON 格式输出到 stderr。
