# Plan 0504 Plus: Iota Context Fabric 可执行增强版

## 0. 目标

在 iota-sympantos Rust 工程中实现统一 Context/Memory/Skill 能力，使 Claude Code、Codex、Gemini CLI、Hermes、OpenCode 五个 ACP backend 共享同一套 iota-owned context fabric。

核心原则：

- ACP 是传输层，不是能力模型。
- iota 是 memory、skill、session、approval、event 的 source of truth。
- backend 原生 memory/skill/config 只能作为可选投影，不作为 canonical store。
- 不修改 backend 源码。
- 不覆盖 `HERMES_HOME`。
- 所有路径、命令、缓存、配置必须跨 Windows/macOS/Linux。

必须覆盖的三项主能力：

1. 记忆分类：`semantic / episodic / procedural` + `identity / preference / strategic / domain` + `session / project / user / global`。
2. 7 种 fn 引擎通过 MCP 提供给 skill：Rust、TypeScript、Python、Go、Java、C++、Zig。
3. Skill 分布式加载：全局、项目、配置 root、后续远程 registry cache。

---

## 1. 总体架构

```text
CLI/TUI/auto daemon
  -> IotaEngine
      -> RuntimeEvent mapper
      -> EventStore / ExecutionRecord
      -> ContextEngine
      -> MemoryStore
      -> DialogueBuffer / WorkingMemory
      -> SkillRegistry
      -> EngineRunSkillExecutor
      -> McpClient / McpRouter
      -> ContextMcpSidecar
      -> ApprovalBroker
      -> SessionLedger
      -> Backend client pool
          -> AcpClient
              -> claude-code / codex / gemini / hermes / opencode
```

### 1.1 用户命令口径

公开命令：

```bash
iota                    # 进入 TUI
iota check              # 输出合并 JSON 信息
iota run <backend> ...  # 一次性执行
iota run --daemon ...   # daemon 路由，自动静默启动
iota bench-cold ...
iota bench-warm ...
```

内部协议/实现名：

- ACP 仍在 `src/acp.rs` 内部实现。
- `__daemon` 是隐藏内部入口。
- `context-mcp` 未来可以作为 MCP server 入口，但不是普通 prompt 命令。

---

## 2. Phase 1a: RuntimeEvent 最小归一化

### 2.1 目标

先建立统一事件类型，不改变当前执行语义，不引入持久化和锁。

### 2.2 新增文件

- `src/runtime_event.rs`

### 2.3 事件类型

```rust
pub enum RuntimeEvent {
    Output(OutputEvent),
    State(StateEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    Error(ErrorEvent),
    Extension(ExtensionEvent),
    Memory(MemoryEvent),
    ApprovalRequest(ApprovalRequestEvent),
    ApprovalDecision(ApprovalDecisionEvent),
}
```

### 2.4 改动点

- 在 `acp.rs` 中新增 ACP wire event -> `RuntimeEvent` 的 mapper。
- 保留当前 `prompt_with_cwd_timed()` 返回 String 的路径。
- 只在内部收集/调试事件，不改变用户输出。

### 2.5 验收

- `cargo build` 通过。
- `iota run codex "ping"` 仍能返回文本。
- `--show-native` 行为不变。
- 单测或调试 helper 能把常见 ACP `session/update` 映射为 `RuntimeEvent::Output`。

---

## 3. Phase 1b: EventStore 记录，不做 replay/join

### 3.1 目标

先把事件写下来，避免一上来实现复杂并发 join/replay。

### 3.2 新增文件

- `src/event_store.rs`

### 3.3 存储选型

优先：SQLite via `rusqlite`。

建议依赖：

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

理由：

- 跨平台稳定。
- 无外部服务。
- 后续可加 FTS5/embedding。

如果 `bundled` 在目标平台构建有问题，降级为 JSONL store 作为 Phase 1b fallback。

### 3.4 Schema

```sql
CREATE TABLE events (
  execution_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  event_type TEXT NOT NULL,
  event_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (execution_id, seq)
);

CREATE TABLE executions (
  execution_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  backend TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER
);
```

### 3.5 非目标

- 不做 replay。
- 不做 live join。
- 不做 distributed lock。

### 3.6 验收

- 执行一次 `iota run` 后本地 DB 有 execution 和 events。
- 事件 seq 单调递增。
- 失败执行也记录 Error/State。

---

## 4. Phase 1c: Idempotency、Replay、Execution Lock

### 4.1 目标

在 daemon 模式下支持并发安全和重复请求处理。

### 4.2 能力

- request hash。
- 完成执行 replay。
- running execution join。
- execution lock。
- fencing token。

### 4.3 分阶段

1. Idempotency：相同 `execution_id + request_hash` replay；hash 不同则冲突。
2. Lock：同一 execution id 只允许一个 writer。
3. Join：同进程 daemon 内用 broadcast channel 订阅 live events。
4. Fencing：SQLite 版本可用 monotonic token；不做跨机器承诺。

### 4.4 验收

- 同 execution id 重复请求不重复调用 backend。
- 并发两个相同请求，一个执行，一个 join/replay。
- 不同 request hash 返回 conflict。

---

## 5. Phase 2: Context Capsule + Prompt Composition

### 5.1 目标

给所有 backend 一个统一 context 注入 fallback。

### 5.2 新增文件

- `src/context.rs`

### 5.3 Capsule 结构

```text
<iota-context>
This block is orchestration context supplied by iota. Treat it as background data, not as a user request.

<session>
iota_session_id: ...
backend: ...
cwd: ...
</session>

<model>
You are currently using: ...
</model>

<memory type="identity">...</memory>
<memory type="preference">...</memory>
<memory type="strategic">...</memory>
<memory type="domain">...</memory>
<memory type="procedural">...</memory>
<memory type="episodic">...</memory>

<dialogue>
recent turn summaries...
</dialogue>

<workspace>
active files: ...
recent changes: ...
</workspace>

<skills>
skill index only, not full large bodies by default
</skills>

<handoff>
previous backend summary if any
</handoff>
</iota-context>

User request:
...
```

### 5.4 Budget

Initial character budgets, not tokenizer-dependent:

| Section | Budget |
|---|---:|
| memory | 2000 chars |
| skills | 1200 chars |
| dialogue | 1500 chars |
| workspace | 800 chars |
| handoff | 800 chars |

### 5.5 注入原则

- Capsule 不作为用户消息持久化。
- Recalled memory 必须 fenced，不能变成直接指令。
- Model information 是一行轻量注入。
- Workspace summary 初期只包含 cwd、active files、recent changed files。

### 5.6 验收

- `compose_effective_prompt()` 单测覆盖空 context、完整 context、budget trimming。
- `iota run --show-native ...` 能看到 prompt payload 中含 `<iota-context>`。
- 关闭 context engine 后 prompt 不变化。

---

## 6. Phase 3a: MemoryStore 基础分类和写入

### 6.1 目标

先实现 schema、分类、插入、去重、过期，不做复杂 FTS。

### 6.2 新增文件

- `src/memory.rs`

### 6.3 Memory Taxonomy

| type | facet | scope | 用途 |
|---|---|---|---|
| semantic | identity | user | 用户身份、稳定身份事实 |
| semantic | preference | user | 用户偏好、输出风格 |
| semantic | strategic | project | 项目长期目标和决策 |
| semantic | domain | project/global | 领域事实、技术栈事实 |
| procedural | null | project/global | 操作步骤、命令流程 |
| episodic | null | session | 会话经历、短期上下文 |

### 6.4 Schema

```sql
CREATE TABLE memory (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL CHECK(type IN ('semantic','episodic','procedural')),
  facet TEXT CHECK(facet IN ('identity','preference','strategic','domain')),
  scope TEXT NOT NULL CHECK(scope IN ('session','project','user','global')),
  scope_id TEXT NOT NULL,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 1.0,
  source_backend TEXT,
  source_session_id TEXT,
  source_execution_id TEXT,
  metadata_json TEXT,
  ttl_days INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  supersedes TEXT,
  owner TEXT NOT NULL DEFAULT 'local',
  visibility TEXT NOT NULL DEFAULT 'private'
);

CREATE UNIQUE INDEX idx_memory_dedup
  ON memory(scope, scope_id, type, facet, content_hash);

CREATE INDEX idx_memory_recall_semantic
  ON memory(scope, scope_id, facet, confidence DESC, updated_at DESC)
  WHERE type = 'semantic';

CREATE INDEX idx_memory_recall_procedural
  ON memory(scope, scope_id, confidence DESC, updated_at DESC)
  WHERE type = 'procedural';

CREATE INDEX idx_memory_recall_episodic
  ON memory(scope, scope_id, created_at DESC)
  WHERE type = 'episodic';
```

### 6.5 污染防护

默认自动提取只写 `episodic/session`。

写 `semantic` 的条件：

- 用户明确说“记住/remember/save this”。
- 或 backend 通过 `iota_memory_write` MCP tool 显式写入。
- 或未来 approval 允许的 LLM-assisted extraction。

写 `identity/preference` 初期必须用户明确表达。

### 6.6 验收

- 插入六类 fixture memory。
- content hash 去重有效。
- expires_at 过滤有效。
- semantic/procedural/episodic 三种基本查询通过。

---

## 7. Phase 3b: 6 桶 Recall + Dialogue Buffer

### 7.1 目标

实现 TS iota 的 6 桶 recall 和短期多轮会话上下文。

### 7.2 6 桶 Recall

| Bucket | Query |
|---|---|
| identity | user semantic identity, limit 20, min confidence 0.85 |
| preference | user semantic preference, limit 30, min confidence 0.80 |
| strategic | project semantic strategic, limit 30, min confidence 0.80 |
| domain | project semantic domain, limit 50, min confidence 0.80 |
| procedural | project procedural, limit 10, min confidence 0.75 |
| episodic | session episodic, limit 20, min confidence 0.70 |

### 7.3 Dialogue Buffer

- In-memory last N turns, default 50。
- 每轮追加 prompt/output summary。
- 每轮结束写一条 `episodic/session` memory，TTL 7 天。
- Backend switching 时生成 handoff memory。

### 7.4 验收

- `recall_buckets()` 返回六个 bucket。
- DialogueBuffer 最近 N 轮裁剪正确。
- Capsule 中按 bucket 输出 memory section。

---

## 8. Phase 3c: FTS5 / 搜索增强

### 8.1 目标

增强 domain/procedural 大量记忆召回。

### 8.2 Gating

只有确认 bundled SQLite 支持 FTS5 后启用。

### 8.3 Fallback

如果 FTS5 不可用：使用 `LIKE` + recency/confidence 排序。

### 8.4 验收

- FTS5 可用时 domain/procedural 能按 query 搜索。
- FTS5 不可用时不影响 build/run。

---

## 9. Phase 4a: Skill Registry 分布式加载

### 9.1 新增文件

- `src/skills.rs`

### 9.2 Roots

优先级从高到低：

1. `<workspace>/.iota/skills/`
2. `context_engine.skill_roots` 中的 project roots
3. `~/.i6/skills/`
4. remote registry cache（后续）

### 9.3 格式

```yaml
---
name: code-review
version: 1
summary: Code review checklist
description: Review code for correctness, security, maintainability
triggers:
  - review code
  - 代码审查
backends: [claude-code, codex, gemini, hermes, opencode]
execution:
  mode: advisory       # advisory | mcp
  server: iota-fun
  parallel: true
  tools: []
output:
  template: ""
failurePolicy: report
---

Skill body...
```

### 9.4 冲突规则

- 同名 skill 高优先级 root 覆盖低优先级 root。
- 同 root 同名冲突：记录 diagnostics，保留第一个稳定排序项。
- 无效 YAML 不阻塞整个 registry。
- `backends` 缺省表示 all。

### 9.5 热加载

- 每次 prompt 前检查 roots mtime。
- mtime 不变复用缓存。
- 加载错误进入 diagnostics。

### 9.6 验收

- 全局和项目 root 都能加载。
- 项目同名 skill 覆盖全局 skill。
- backend filtering 生效。
- Capsule 中只注入兼容 skill 的 index。

---

## 10. Phase 4b: Engine-run deterministic skill execution

### 10.1 目标

Skill 命中 trigger 后，engine 自己执行 MCP tool plan，不交给 backend model 决定。

### 10.2 新增文件

- `src/mcp_client.rs`
- `src/skill_runner.rs`

### 10.3 执行流程

```text
prompt -> SkillRegistry::match_skill()
  -> if execution.mode == mcp
  -> SkillRunner builds tool calls
  -> McpClient calls configured server tools
  -> collect tool results
  -> render output.template
  -> RuntimeEvent::ToolCall/ToolResult/Output
  -> no backend model call
```

### 10.4 验收

- 创建 trigger skill。
- `iota run codex "trigger"` 不调用 backend model，直接输出 template 结果。
- tool call/result event 被记录。
- `parallel: true` 并行执行。

---

## 11. Phase 4c: 7 种 fn 引擎通过 MCP 提供给 skill

### 11.1 目标

实现 `iota-fun` MCP server，暴露 7 种语言工具。

### 11.2 Tools

| MCP Tool | Language | Execution |
|---|---|---|
| `fun.rust` | Rust | compile + cache |
| `fun.typescript` | TypeScript | bun/node direct |
| `fun.python` | Python | python direct |
| `fun.go` | Go | go run |
| `fun.java` | Java | javac + java |
| `fun.cpp` | C++ | clang++/g++ compile + cache |
| `fun.zig` | Zig | zig run/build |

### 11.3 Security model

Default restrictions:

- No secret env forwarding.
- Explicit allowlist of executable paths from config or PATH lookup。
- Per tool timeout, default 10s。
- Output max bytes, default 64 KiB。
- Cache dir under `~/.i6/fun-cache/` via `dirs::home_dir()`。
- Cache key includes source hash, language, compiler version if available。
- Missing compiler returns structured tool error, not panic。
- Network disabled by convention initially; stronger sandbox later。
- Approval required when tool declares shell/network/file-write behavior。

### 11.4 验收

- `iota context-mcp` exposes tools/list with 7 tools。
- 每个 tool 能执行 sample function。
- 编译型第二次命中 cache。
- 缺少 Zig/Java/C++ 时返回 clear error。
- Skill 使用两个以上 fn tools 能合成 output.template。

---

## 12. Phase 5a: Context MCP Sidecar

### 12.1 新增文件

- `src/context_mcp.rs`

### 12.2 Tools

| Tool | Purpose |
|---|---|
| `iota_memory_search` | search unified memory |
| `iota_memory_write` | write unified memory |
| `iota_skill_search` | search skill index |
| `iota_skill_load` | load full skill body |
| `iota_session_summary` | read session summary |
| `iota_handoff_publish` | publish handoff |
| `iota_handoff_read` | read handoff |

### 12.3 Resources

- `iota://memory/{scope}/{scope_id}`
- `iota://skill/{name}`
- `iota://session/{id}/summary`
- `iota://workspace/{id}/rules`

### 12.4 ACP mcpServers 注入能力表

| Backend | `session/new.mcpServers` | Notes |
|---|---|---|
| Gemini | yes | first target |
| OpenCode | yes | always send array, even empty |
| Hermes | maybe | convert env to `string[]`; do not set `HERMES_HOME` |
| Claude adapter | try | fallback capsule |
| Codex adapter | try | fallback capsule |

### 12.5 验收

- `iota context-mcp` can respond to MCP initialize/tools/list/tools/call。
- Gemini/OpenCode receive mcpServers in `session/new`。
- Unsupported backend still works through prompt capsule。

---

## 13. Phase 5b: MCP response channel, gated

### 13.1 目标

Engine 拦截 backend MCP tool call，代为执行并回传结果。

### 13.2 Gating

必须先为每个 backend 验证：

- 是否能把 MCP tool call 映射成 RuntimeEvent::ToolCall。
- 是否支持 engine 写回 tool_result。
- 写回失败时是否能 fail fast，避免 backend hang。

### 13.3 验收

- 至少 Gemini 或 OpenCode 一个 backend 完整验证。
- 拦截 `iota_memory_search`，engine 执行，结果回传 backend。
- Approval policy 能拦截 `mcp_external`。

---

## 14. Phase 6: Approval 归一化与持久化

### 14.1 新增文件

- `src/approval.rs`

### 14.2 流程

```text
backend permission request
  -> RuntimeEvent::ApprovalRequest
  -> persist request
  -> policy decision / user prompt
  -> persist decision
  -> send response to backend
```

### 14.3 Policy dimensions

- shell
- file_outside_workspace
- network
- mcp_external
- privilege_escalation

### 14.4 验收

- request before decision ordering is persisted。
- denied decision returns tool_result/error to backend where supported。
- TUI still handles interactive permission request。

---

## 15. Phase 7: SessionLedger + Backend switching

### 15.1 新增文件

- `src/session_ledger.rs`

### 15.2 Schema

```sql
CREATE TABLE sessions (
  iota_session_id TEXT PRIMARY KEY,
  cwd TEXT NOT NULL,
  active_backend TEXT,
  model TEXT,
  turn_count INTEGER DEFAULT 0,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL
);

CREATE TABLE backend_sessions (
  iota_session_id TEXT NOT NULL,
  backend TEXT NOT NULL,
  backend_session_id TEXT,
  cwd TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL,
  PRIMARY KEY (iota_session_id, backend, cwd)
);

CREATE TABLE turns (
  turn_id TEXT PRIMARY KEY,
  iota_session_id TEXT NOT NULL,
  backend TEXT NOT NULL,
  execution_id TEXT,
  prompt_hash TEXT,
  output_summary TEXT,
  status TEXT,
  started_at INTEGER,
  finished_at INTEGER
);
```

### 15.3 Switching protocol

1. Summarize recent dialogue and workspace changes。
2. Store as episodic handoff memory。
3. Start/reuse target backend session。
4. Inject `<handoff>` capsule on first target prompt。

### 15.4 验收

- Same iota session can use two backends。
- Target backend receives handoff capsule。
- Dialogue continuity survives backend switch。

---

## 16. Phase 8: Native materializer, optional and conservative

### 16.1 目标

将 canonical iota memory/skill 投影到 backend 原生文件，只作为 UX/compat layer。

### 16.2 Rules

- Default off。
- Dry-run first。
- Never overwrite user content。
- Use `<!-- IOTA_START -->` / `<!-- IOTA_END -->` blocks。
- No hardcoded backend home assumptions。
- Hermes native materialization is deferred until current Hermes source confirms external skill path。

### 16.3 验收

- Claude/Gemini/OpenCode at least one native projection dry-run。
- Running materializer twice is idempotent。
- User content outside iota block unchanged。

---

## 17. Phase 9: Config 扩展

### 17.1 `nimia.yaml`

```yaml
context_engine:
  enabled: true
  injection: auto           # auto | prompt | mcp | native | off
  memory_db: ~/.i6/context/memory.sqlite
  skill_roots:
    - ~/.i6/skills
    - ./.iota/skills
  native_overlays: false
  budgets:
    memory_chars: 2000
    skills_chars: 1200
    dialogue_chars: 1500
    workspace_chars: 800
  mcp:
    enabled: true
    name: iota-context
    command: iota
    args: ["context-mcp"]
  fun:
    enabled: true
    name: iota-fun
    command: iota
    args: ["fun-mcp"]
    cache_dir: ~/.i6/fun-cache

context_engine_backend:
  gemini:
    mcp_session_new: true
  opencode:
    mcp_session_new: true
    always_send_empty_mcp_servers: true
  hermes:
    mcp_session_new: true
    mcp_env_shape: string_array
    override_home: false
  claude-code:
    mcp_session_new: try
  codex:
    mcp_session_new: try
```

### 17.2 验收

- Missing context_engine uses safe defaults。
- `~/` expands through existing home expansion helper。
- Windows path behavior tested manually or with unit tests。

---

## 18. Verification Matrix

| Capability | Minimal verification |
|---|---|
| RuntimeEvent | ACP update maps to Output/ToolCall/Error |
| EventStore | execution/events rows written after run |
| Context Capsule | show-native contains `<iota-context>` |
| Memory taxonomy | 6 fixture buckets inserted/recalled |
| Dialogue buffer | last N turns retained and trimmed |
| Skill distributed load | workspace skill overrides global skill |
| Engine-run skill | trigger returns template output without backend model call |
| 7 fn tools | tools/list has 7 tools, sample execution works |
| MCP sidecar | tools/list and memory_search work via stdio JSON-RPC |
| MCP response channel | one verified backend can receive engine tool_result |
| Approval | request and decision persisted in order |
| Session ledger | backend switch injects handoff |
| Native materializer | dry-run idempotent, no user content overwrite |

---

## 19. Open Risks

1. FTS5 availability on all targets。
2. MCP result writeback differences across backend adapters。
3. 7 language toolchains missing on user machines。
4. Skill execution security without OS sandbox。
5. Automatic memory extraction pollution。
6. Native materializer path drift across backend versions。

Mitigation: keep features gated, default to prompt capsule, prefer explicit user-triggered writes for durable memory, and verify backend capability one backend at a time.
