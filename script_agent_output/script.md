# Video Script: Multi-Agent AI Coding Orchestration

> **Topic source:** Research Agent → Supabase (placeholder — data source not yet connected)  
> **Heat score:** 85/100  
> **Output format:** Structured JSON + Markdown, ready for X optimization

---

## Hook (0:00-0:20)

**[On Screen: Split comparison — single API call vs multi-backend parallel]**

> **VO:** 90% 的开发团队还在用单一大模型 API——其实你早该让多个 AI 编程助手同时协作了。

---

## Narrative Structure

### Phase 1 — 痛点引入 (0:20-0:40)
单后端方案的三个固有问题：
- 响应延迟高（单路往返）
- 上下文窗口耗尽快
- 质量不稳定（模型本身的随机性）

### Phase 2 — 方案展示 (0:40-1:10)
iota-sympantos 的 ACP 协议架构：

```
User Prompt
    ↓
ACP Protocol (JSON-RPC 2.0 over stdin/stdout)
    ↓
┌──────────┬───────────┬────────────┬──────────────┬──────────┐
│Claude    │Codex      │Gemini CLI  │Hermes Agent  │OpenCode  │
│Code      │           │            │              │          │
└──────────┴───────────┴────────────┴──────────────┴──────────┘
    ↓           ↓            ↓              ↓            ↓
         Unified Context Fabric + Memory Store
                      ↓
              Result + Memory
```

### Phase 3 — 深度演示 (1:10-1:50)
**TUI 实时切换后端 + Kanban 任务流转**

- 运行 `iota` 启动 TUI
- Kanban board 展示 running/ready 任务
- 运行时无缝切换后端（Ctrl+T）
- Context Fabric 记忆跨 session 保持

**On Screen text:**
```
Backend: Claude Code  [←]  Gemini CLI  [→]
Status: running  |  Token: 12,340
```

### Phase 4 — 价值升华 (1:50-2:05)
团队视角：每个后端擅长不同任务——Codex 强在代码补全，Gemini CLI 强在信息检索，Hermes 强在记忆管理。通过 ACP 协议和 Context Fabric，它们像一个大脑一样协作。

---

## Voiceover Script

### Opening (0:00-0:20)
> 你还在用一个 AI 编程工具吗？今天我用一个 Rust 写的编排框架，让你同时调动 5 个主流 AI 编程后端——Claude Code、Codex、Gemini CLI、Hermes Agent，还有一个开源的 OpenCode。

### Body (0:20-1:50)
> 它的核心是一个叫 ACP 的协议，基于 JSON-RPC 2.0，通过 stdin 和 stdout 和每个后端进程通信。这意味着：不管模型怎么升级，只要它支持这个协议，就能无缝接入。
>
> 看看实际效果。这是我在本届 Kanban board 上的任务列表，ACP 协议会自动把任务分发给我选择的任意后端。你看，我可以让 Codex 处理复杂重构，同时让 Gemini CLI 做信息检索，它们互不干扰，但结果统一写回同一个 Memory Store。
>
> 这就是多后端协作真正有价值的地方——每个模型擅长不同类型的工作，通过一个统一的上下文层把它们串联起来，你不需要在多个工具之间来回切换。

### Closing (1:50-2:15)
> iota-sympantos 完全开源，用 Rust 实现，性能很高。项目描述里有快速上手链接，有问题可以在 GitHub 提 issue。觉得这期有用的话，点赞、订阅、打开小铃铛，我是 Han，我们下期见。

---

## On-Screen Callouts

| Element | Text |
|---------|------|
| Title card | **ACP = Agent Control Protocol** |
| Backend list | Claude Code · Codex · Gemini CLI · Hermes Agent · OpenCode |
| CTA URL | github.com/hanl5/iota-sympantos |

---

## CTA Section

```
_primary_   Star on GitHub  →  github.com/hanl5/iota-sympantos
_secondary_ Read the docs  →  github.com/hanl5/iota-sympantos/blob/main/docs/iota%20book.md
```

---

## X/Twitter Optimization Tags (ready for copy-paste)

```
#AIcoding #MultiAgent #Rustlang #ClaudeCode #Codex #GeminiCLI #HermesAgent #OpenCode
#ACPprotocol #IotaSympantos #DeveloperTools #AIAssistants
```

---

## Metadata

| Field | Value |
|-------|-------|
| Created by | script_agent (kanban task 6) |
| Heat score | 85 |
| Status | **Placeholder** — real topics require Research Agent Supabase integration |
| Ready for X optimization | ✅ true |
| JSON output | `script_agent_output/script.json` |
| Markdown output | `script_agent_output/script.md` |