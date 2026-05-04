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
│   ├── cli.rs               # 命令分发（run/check/tui/bench 等）
│   ├── tui.rs               # 交互式 TUI 主循环
│   ├── tui/
│   │   ├── composer.rs      # 多行输入组件（kill buffer/Ctrl+R/word motion）
│   │   ├── markdown.rs      # markdown 渲染（pulldown-cmark）
│   │   ├── status_bar.rs    # 底部状态栏（后端·模型 / 快捷键提示）
│   │   ├── theme.rs         # ratatui 颜色主题（洋红主色）
│   │   └── state.rs         # TUI 状态
│   ├── engine.rs            # ACP 运行时编排，客户端池
│   ├── acp.rs              # ACP JSON-RPC 2.0 协议驱动
│   ├── agent.rs            # 内部 daemon（127.0.0.1:47661）
│   ├── config.rs           # nimia.yaml 配置解析
│   ├── runtime_event.rs    # 统一事件类型（Output/ToolCall/Approval 等）
│   ├── event_store.rs      # SQLite 事件持久化
│   ├── memory.rs           # MemoryStore（6 桶分类体系）
│   ├── context.rs          # ContextEngine + capsule 组装 + budget
│   ├── skills.rs           # SkillRegistry（分布式加载 + trigger 匹配）
│   ├── skill_runner.rs     # engine-run skill 执行
│   ├── mcp_client.rs       # engine 侧 MCP 客户端
│   ├── mcp_router.rs       # MCP 工具调用拦截路由
│   ├── context_mcp.rs      # iota-context MCP sidecar（stdio）
│   ├── fun_mcp.rs         # iota-fun 7 语言 MCP server（stdio）
│   ├── approval.rs         # ApprovalStore + policy
│   ├── session_ledger.rs  # SessionLedger + 后端切换 handoff
│   └── native_materializer.rs # 原生文件投影（可选）
├── doc/
│   ├── plan-0504.md        # Context Fabric 完整规划
│   └── plan-0504-plus.md  # Context Fabric 增强版规划
├── Cargo.toml
└── ~/.i6/nimia.yaml       # 唯一配置来源
```

---

## ACP 协议流程

每个后端都是通过 `npx`（或 `hermes acp`）启动的外部进程，协议为基于 stdin/stdout 的换行分隔 JSON-RPC 2.0：

```
initialize → session/new → session/prompt → 流式 session/update → session/complete
```

执行路径：
- **直接路径**：`IotaEngine::prompt_in_cwd`，按需启动并复用 ACP 客户端
- **Daemon 路径**：通过 `IotaEngine` 经内部 daemon（`--daemon` / `-d`）路由

---

## 后端适配器

| 后端 | 命令 | 别名 |
|------|------|------|
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

运行时通过 `backend_process_env()` 将 model 配置映射为各后端所需的环境变量：
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
iota native-materialize  # 将 memory/skill 投影到原生文件
iota skill pull <源> [名称]
iota __daemon           # 内部 daemon 入口
```

---

## TUI 功能（已完成）

| 功能 | 文件 | 状态 |
|------|------|------|
| 多行输入（Shift+Enter 换行） | `tui/composer.rs` | ✅ |
| Unicode grapheme 光标 | `tui/composer.rs` | ✅ |
| Kill buffer（Ctrl+K/Ctrl+Y） | `tui/composer.rs` | ✅ |
| Ctrl+U/Ctrl+W 词删除 | `tui/composer.rs` | ✅ |
| Alt+B/Alt+F 词间移动 | `tui/composer.rs` | ✅ |
| Ctrl+R 增量历史搜索 | `tui/composer.rs` | ✅ |
| Markdown 渲染 | `tui/markdown.rs` | ✅ |
| 状态栏（洋红主色，后端·模型） | `tui/status_bar.rs` | ✅ |
| 运行指示器（spinner + 耗时） | `tui.rs` | ✅ |
| Ctrl+T 全屏 pager | `tui.rs` | ✅ |
| ? 帮助浮层 | `tui.rs` | ✅ |
| 二次 Ctrl+C 退出确认 | `tui.rs` | ✅ |
| Esc 中断运行中任务 | `tui.rs` | ✅ |
| Tab 队列（运行时缓存输入） | `tui.rs` | ✅ |
| 浮层枚举（None/Help/Pager/QuitConfirm） | `tui.rs` | ✅ |

### TUI 仍缺失功能

| 功能 | 优先级 | 说明 |
|------|--------|------|
| 恐慌钩子（崩溃前恢复终端） | P0 | 暂无 `set_panic_hook()` |
| 错误路径终端恢复 | P0 | `?` 提前返回会跳过清理 |
| is-terminal 检查 | P0 | 未检查 stdin/stdout 是否为终端 |
| Engine turn 移出主任务 | P0 | 阻塞绘制循环 |
| Approval 浮层 | P1 | `session/request_permission` 被静默丢弃 |
| 帧率限制器（120 FPS） | P1 | 无 MIN_FRAME_MS 节流 |
| 流式输出（增量渲染） | P3 | 仍等待完整文本一次性渲染 |
| 鼠标滚轮 | P2 | 事件未处理 |
| 光标隐藏 | P2 | 光标在 TUI 上闪烁 |
| 键盘增强标志 | P2 | Shift+Enter 在部分终端可能失效 |
| Ctrl+D / EOF 处理 | P2 | 清理路径可能被跳过 |
| 窗口标题（OSC） | P3 | 未实现 |
| 外部编辑器（Ctrl+X） | P3 | 未实现 |

---

## Context Fabric 实现状态（对照 plan-0504 / plan-0504-plus）

| Phase | 内容 | 文件 | 状态 |
|-------|------|------|-------|
| 1 | RuntimeEvent 归一化 | `runtime_event.rs` | ✅ |
| 1 | EventStore SQLite 持久化 | `event_store.rs` | ✅ |
| 1 | Execution idempotency + lock + fencing | `event_store.rs` | ✅ |
| 2 | Context Capsule + budget | `context.rs` | ✅ |
| 3 | MemoryStore（6 桶分类） | `memory.rs` | ✅ |
| 3 | 6 桶 Recall 查询 | `memory.rs` | ✅ |
| 3 | DialogueBuffer | `context.rs` | ✅ |
| 4 | SkillRegistry 分布式加载 | `skills.rs` | ✅ |
| 4 | Skill trigger 匹配 | `skills.rs` | ✅ |
| 4b | Engine-run skill execution | `skill_runner.rs` | ✅ |
| 4b | 7 种 fn 引擎（iota-fun MCP） | `fun_mcp.rs` | ✅ |
| 4b | MCP client | `mcp_client.rs` | ✅ |
| 5a | MCP sidecar（iota-context） | `context_mcp.rs` | ✅ |
| 5a | ACP mcpServers 注入 | `acp.rs` | ✅ |
| 5b | MCP response channel / 拦截 | `mcp_router.rs` | ✅ |
| 6 | Approval 归一化 + 持久化 | `approval.rs` | ✅ |
| 7 | SessionLedger + handoff | `session_ledger.rs` | ✅ |
| 8 | Native materializer | `native_materializer.rs` | ✅ |
| 9 | Config 扩展（context_engine） | `config.rs` | ✅ |

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

## 安全要求

- 绝不提交 API 密钥、Token、密码或任何敏感信息
- `nimia.yaml` 包含后端凭据，禁止提交到版本控制
- 文档和调试输出中保持敏感信息打码
- `--show-native` 可能暴露敏感协议内容，仅用于本地调试

---

## 新增后端步骤

1. 在 `acp.rs` 的 `AcpBackend` 枚举中添加变体
2. 实现 `parse()`、`command()`、`Display` 分支
3. 加入 `ALL_BACKENDS`
4. 在 `config.rs` 的 `NimiaConfig` 和 `BackendConfig` 中添加字段
5. 在 `backend_config()`、`backend_home_env_key()`、`backend_process_env()` 中添加分支
6. 在 `nimia.yaml.template` 中添加后端配置段
