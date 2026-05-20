# AGENTS.md

## 语言约束

本文档及所有代码注释、提交信息、产出物 **只能使用中文或英文**，禁止使用韩语及其他语言。

---

## 项目概述

iota-sympantos 是一个轻量级 Rust CLI，通过 ACP（Agent Control Protocol）协议编排多个 AI 编程助手后端。支持单次执行和交互式 TUI 两种模式，支持 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 五个后端。

---

## 源码结构

```
iota-sympantos/
├── src/
│   ├── main.rs              # 程序入口
│   ├── cli/
│   │   └── mod.rs           # 命令分发（run/check/tui/bench 等）
│   ├── tui.rs               # 交互式 TUI 主循环
│   ├── tui/
│   │   ├── input.rs        # 多行输入组件（kill buffer/Ctrl+R/word motion）
│   │   ├── scrollback.rs   # 终端回滚缓冲（insert_before）
│   │   ├── loop.rs         # TUI 主事件循环
│   │   ├── markdown.rs      # markdown 渲染（pulldown-cmark）
│   │   ├── status_bar.rs    # 底部状态栏（后端·模型 / 快捷键提示）
│   │   ├── theme.rs         # ratatui 颜色主题（洋红主色）
│   │   └── state.rs         # TUI 状态
│   ├── engine.rs            # IotaEngine 编排，ACP client pool、context、memory、skill、ledger
│   ├── acp/
│   │   ├── mod.rs           # ACP JSON-RPC 2.0 协议驱动、AcpClient
│   │   ├── permission.rs    # 权限请求处理（iota 工具自动批准）
│   │   ├── session.rs       # session/new 参数渲染、mcpServers shape
│   │   └── wire.rs          # line read/parse、response id 匹配
│   ├── daemon/
│   │   ├── mod.rs           # 内部 daemon TCP server（127.0.0.1:47661）
│   │   ├── pool.rs          # EnginePool（按 cwd 维度复用 IotaEngine）
│   │   └── proto.rs         # DaemonPromptRequest/Response wire types
│   ├── config.rs            # nimia.yaml 配置解析 + per-backend context options
│   ├── runtime_event.rs     # 统一事件类型（Output/ToolCall/Approval 等）
│   ├── store/
│   │   ├── mod.rs           # Store layer 入口
│   │   ├── cache.rs         # CacheStore execution replay / dedupe
│   │   ├── memory.rs        # MemoryStore（6 桶分类体系）
│   │   ├── approvals.rs     # ApprovalStore + policy
│   │   └── ledger.rs        # SessionLedger + 后端切换 handoff
│   ├── context/
│   │   └── mod.rs           # ContextEngine、WorkingMemoryBuffer、capsule 组装 + budget
│   ├── kanban/
│   │   ├── mod.rs           # 模块入口 + re-exports
│   │   ├── types.rs         # Task/Board/Run/Comment/Link 领域类型
│   │   ├── store.rs         # KanbanStore trait（CRUD + event sourcing 接口）
│   │   ├── sqlite_store.rs  # SqliteKanbanStore 实现（event-sourced）
│   │   ├── state_machine.rs # 状态机（triage→todo→ready→running→done→archived + blocked）
│   │   ├── event_sourcing.rs# Event replay、apply_event
│   │   ├── dispatcher.rs    # Dispatcher — 调度 ready 任务给 hermes worker
│   │   ├── worker.rs        # WorkerHandle — spawn/kill hermes -z 进程
│   │   ├── shadow.rs        # ShadowMaterializer + ShadowWatcher（投影 + 回收）
│   │   ├── bridge.rs        # AdvancedBridge（decompose/specify 编排）
│   │   └── event_sync.rs    # 跨节点事件同步（export/import/serve/pull/push）
│   ├── skill/
│   │   ├── mod.rs           # SkillRegistry（分布式加载 + trigger 匹配）
│   │   ├── runner.rs        # engine-run skill 执行
│   │   ├── cache.rs         # skill pull/cache（HTTP 或本地）
│   │   └── fun.rs           # iota-fun 7 语言 MCP server（stdio）
│   ├── mcp/
│   │   ├── mod.rs           # MCP 层入口
│   │   ├── client.rs        # engine 侧 MCP 客户端
│   │   ├── server.rs        # iota-context MCP stdio server（JSON-RPC 协议适配）
│   │   ├── router.rs        # ACP tool-call 拦截，委托 tool_dispatch
│   │   └── tool_dispatch.rs # 共享工具派发逻辑（解析器、验证器、handlers）
│   ├── native/
│   │   └── mod.rs           # 原生文件投影（可选）
│   └── utils.rs             # 共享工具函数
├── docs/
│   ├── architecture.md      # 分层架构和模块职责
│   ├── code-call-chains.md  # 入口、IPC 和调用链
│   └── observability.md     # logs/trace、RuntimeEvent、metrics、CacheStore 边界
├── gefsi/
│   └── exp03-acp-runtime.md # ACP 进程模型和 benchmark 验证报告
├── Cargo.toml
└── ~/.i6/nimia.yaml         # 唯一配置来源
```

---

## ACP 协议流程

每个后端都是通过 `npx`（或 `hermes acp`）启动的外部进程，协议为基于 stdin/stdout 的换行分隔 JSON-RPC 2.0：

```
initialize → session/new → session/prompt → 流式 session/update → session/complete
```

执行路径：

- **直接路径**：`IotaEngine::run_with_timing`，按需启动并复用 ACP 客户端
- **Daemon 路径**：通过 `IotaEngine` 经内部 daemon（`--daemon` / `-d`）路由

---

## 后端适配器

| 后端 | 命令 | 别名 |
| :------| :------| :------|
| Claude Code | `npx` | `claude`, `claudecode` |
| Codex | `npx` | `codex` |
| Gemini CLI | `npx` | `gemini`, `gemini-cli` |
| Hermes Agent | `hermes acp` | `hermes` |
| OpenCode | `npx` | `opencode`, `open-code` |

---

## 配置（nimia.yaml）

配置**仅**从 `~/.i6/nimia.yaml` 读取，无项目级配置或自动发现。

### `model` 字段处理

```yaml
model:
  provider: minimax-cn
  name: MiniMax-M2.7
  base_url: https://api.minimaxi.com/anthropic
  api_key: <api-key>
```

运行时通过 `backend_process_env_with_context()` 将 model 配置映射为各后端所需的环境变量：

- `claude-code`：api_key → `ANTHROPIC_API_KEY` + `ANTHROPIC_AUTH_TOKEN`；base_url → `ANTHROPIC_BASE_URL`；name → `ANTHROPIC_MODEL`
- `codex`：api_key → `OPENAI_API_KEY` + `ROUTER_API_KEY`；base_url → `OPENAI_BASE_URL`；name → `OPENAI_MODEL`
- `gemini`：api_key → `GEMINI_API_KEY`；name → `GEMINI_MODEL`
- `hermes`：api_key/base_url/name/provider → provider 原生环境变量
- `opencode`：name → `OPENCODE_MODEL`

### Hermes 特殊处理

Hermes 使用自己的默认 `HERMES_HOME`（Windows 上 `~/AppData/Local/hermes`，Unix 上 `~/.hermes`）。**不要覆盖 `HERMES_HOME`**。

nimia.yaml 中的 hermes 配置映射为 Hermes 通过 `os.getenv()` 读取的 provider 原生环境变量：

- `provider` → `HERMES_INFERENCE_PROVIDER`
- `name` → `HERMES_MODEL`
- api_key + base_url → `render_hermes_provider_env()` 解析的 provider 相关变量

---

## CLI 命令

```bash
iota                     # 进入 TUI（默认）
iota check [--daemon|-d] # 输出合并的 JSON 后端信息
iota run <backend> ...   # 单次执行
iota run --daemon ...    # 经 daemon 路由，自动静默启动
iota bench-cold [轮次] [--daemon]
iota bench-warm [轮次] [--daemon]
iota context-mcp         # 启动 iota-context MCP sidecar（stdio）
iota fun-mcp            # 启动 iota-fun 7 语言 MCP server（stdio）
iota skill pull <源> [名称]
iota __daemon           # 内部 daemon 入口
```

---

## TUI 功能（已完成）

| 功能 | 文件 | 状态 |
| :------| :------| :------|
| 多行输入（Shift+Enter 换行） | `tui/input.rs` | ✅ |
| Unicode grapheme 光标 | `tui/input.rs` | ✅ |
| Kill buffer（Ctrl+K/Ctrl+Y） | `tui/input.rs` | ✅ |
| Ctrl+U/Ctrl+W 词删除 | `tui/input.rs` | ✅ |
| Alt+B/Alt+F 词间移动 | `tui/input.rs` | ✅ |
| Ctrl+R 增量历史搜索 | `tui/input.rs` | ✅ |
| Markdown 渲染 | `tui/markdown.rs` | ✅ |
| 状态栏（洋红主色，后端·模型） | `tui/status_bar.rs` | ✅ |
| 运行指示器（spinner + 耗时） | `tui.rs` | ✅ |
| Ctrl+T 全屏 pager | `tui.rs` | ✅ |
| ? 帮助浮层 | `tui.rs` | ✅ |
| 二次 Ctrl+C 退出确认 | `tui.rs` | ✅ |
| Esc 中断运行中任务 | `tui.rs` | ✅ |
| Tab 队列（运行时缓存输入） | `tui.rs` | ✅ |
| 浮层枚举（None/Help/Pager/QuitConfirm） | `tui.rs` | ✅ |

### TUI 当前状态

| 功能 | 文件 | 状态 |
| :------| :------| :------|
| Panic hook 终端恢复 | `tui.rs` | ✅ |
| 错误路径终端恢复（RAII guard） | `tui.rs` | ✅ |
| stdout is-terminal 检查 | `tui.rs` | ✅ |
| Engine turn 后台 task 执行 | `tui.rs` | ✅ |
| Approval 浮层 | `tui.rs` / `acp/permission.rs` | ✅ |
| 帧率限制器（约 120 FPS） | `tui.rs` | ✅ |
| 流式输出增量渲染 | `tui.rs` / `engine.rs` / `acp/mod.rs` | ✅ |
| 鼠标捕获启用 | `tui.rs` | ✅ |

### TUI 仍可改进

| 功能 | 优先级 | 说明 |
| :------| :--------| :------|
| 鼠标滚轮滚动 | P2 | 已启用鼠标捕获，但滚轮事件未形成完整滚动交互 |
| 键盘增强标志 | P2 | Shift+Enter 在部分终端仍依赖终端自身支持 |
| 窗口标题（OSC） | P3 | 尚未设置终端窗口标题 |
| 外部编辑器（Ctrl+X） | P3 | 尚未接入 `$EDITOR` / `$VISUAL` |

---

## Context Fabric 实现状态（对照 plan-0504 / plan-0504-plus）

| Phase | 内容 | 文件 | 状态 |
| :-------| :------| :------| :-------|
| 1 | RuntimeEvent 归一化 | `runtime_event.rs` | ✅ |
| 1 | CacheStore execution replay / dedupe | `store/cache.rs` | ✅ |
| 1 | Execution idempotency + lock + fencing | `store/cache.rs` | ✅ |
| 2 | Context Capsule + budget | `context/mod.rs` | ✅ |
| 3 | MemoryStore（6 桶分类） | `store/memory.rs` | ✅ |
| 3 | 6 桶 Recall 查询 | `store/memory.rs` | ✅ |
| 3 | WorkingMemoryBuffer（短期工作记忆） | `context/mod.rs` | ✅ |
| 4 | SkillRegistry 分布式加载 | `skill/mod.rs` | ✅ |
| 4 | Skill trigger 匹配 | `skill/mod.rs` | ✅ |
| 4b | Engine-run skill execution | `skill/runner.rs` | ✅ |
| 4b | 7 种 fn 引擎（iota-fun MCP） | `skill/fun.rs` | ✅ |
| 4b | MCP client | `mcp/client.rs` | ✅ |
| 5a | MCP sidecar（iota-context） | `mcp/server.rs` + `mcp/tool_dispatch.rs` | ✅ |
| 5a | ACP mcpServers 注入 | `acp/session.rs` | ✅ |
| 5b | MCP response channel / 拦截 | `mcp/router.rs` | ✅ |
| 6 | Approval 归一化 + 持久化 | `store/approvals.rs` | ✅ |
| 7 | SessionLedger + handoff | `store/ledger.rs` | ✅ |
| 8 | Config 扩展（context_engine） | `config.rs` | ✅ |

**所有 Phase 均已实现。**

---

## 跨平台要求

**所有代码、配置、路径处理必须同时支持 Windows/macOS/Linux：**

- 使用 `dirs::home_dir()` 解析 home 目录，绝不硬编码 `~`、`%USERPROFILE%` 或 `$HOME`
- `normalize_command()` 在 Windows 上将 `"npx"` 重写为 `"npx.cmd"`
- 文件系统操作使用 `Path`/`PathBuf`，绝不字符串拼接 `\` 或 `/`
- 后端 home 目录因操作系统而异（如 Hermes 在 Windows 上为 `~/AppData/Local/hermes`）
- 进程启动使用 `Stdio::piped()` 和 `kill_on_drop(true)`（tokio 跨平台）
- 配置文件模板中路径使用 `~/` 前缀，由 `expand_home_path()` 在运行时展开
- 在 Windows（主要开发平台）上手动测试后再提交；CI 覆盖 Linux

---

## 单元测试规范

所有 `#[cfg(test)] mod tests {}` 内联测试块必须提取为独立文件，使用 `#[path]` 属性引用：

```rust
// 源文件 (module.rs)
#[cfg(test)]
#[path = "module_tests.rs"]
mod tests;
```

```rust
// 独立测试文件 (module_tests.rs)
use super::*;

#[test]
fn my_test() { ... }
```

**规则：**

- 测试文件命名：`<module_name>_tests.rs`
- 测试文件放在同目录下
- 使用 `use super::*` 导入父模块内容
- 测试函数直接放在文件顶层，不嵌套 `mod inner {}`
- `async fn` 测试使用 `#[tokio::test]` 宏，无需额外配置（自动继承 crate 的 edition）
- 辅助函数（如 `process_exists`）直接放在文件内

**已有规范化的测试文件：**

- `tui/input_tests.rs` — 7 个测试
- `tui/events_tests.rs` — 2 个测试
- `tui/loop_tests.rs` — 3 个测试
- `tui/kanban_view_tests.rs` — 4 个测试
- `context/context_tests.rs`
- `utils/tests.rs`
- `runtime_event/tests.rs`
- `kanban/worker_tests.rs` — 4 个测试

---

## 模块上下文规范（SKILL.md）

每个 `src/<module>/` 目录可包含 `SKILL.md`，作用是让 AI coding 工具快速掌握该模块的代码结构、设计决策和关键类型，而无需阅读全部实现。

**文件格式：**

```yaml
---
name: <skill-name>
description: Use when working on <module> ...
triggers:
  - src/<module>
  - <keyword1>
  - <keyword2>
---

# <module> — 一句话描述职责

## Responsibilities
- 职责1
- 职责2

## Sub-modules
| Module | Purpose |
| :------| :------|
| ... | ... |

## Key Types
- `TypeName` — 描述
```

**规则：**

- `triggers` 字段匹配 AI coding 工具的自动激活条件
- 内容面向 AI 理解，简洁、结构化、避免实现细节
- 人类可读文档放在 `docs/` 目录，不混在 `SKILL.md` 中

---

## 安全要求

- 绝不提交 API 密钥、Token、密码或任何敏感信息
- `nimia.yaml` 包含后端凭据，禁止提交到版本控制
- 文档和调试输出中保持敏感信息打码
- `--show-native` 可能暴露敏感协议内容，仅用于本地调试

---

## 新增后端步骤

1. 在 `acp/mod.rs` 的 `AcpBackend` 枚举中添加变体
2. 实现 `parse()`、`command()`、`Display` 分支
3. 加入 `ALL_BACKENDS`
4. 在 `config.rs` 的 `NimiaConfig` 和 `BackendConfig` 中添加字段
5. 在 `backend_config()`、`backend_home_env_key()`、`backend_process_env_with_context()` 中添加分支
6. 在 `nimia.yaml.template` 中添加后端配置段
