# iota-sympantos experiment 1: cross-backend memory continuity validation

Status note: this is a historical experiment report. Several commands and storage claims refer to the pre-OpenTelemetry `EventStore` / `iota observability` implementation. For current logging, tracing, metrics, and storage locations, see `doc/observability.md`.

| Field | Value |
|-------|-------|
| Experiment ID | exp01-memory |
| Date | 2026-05-07 |
| Working directory | `D:\coding\creative\iota-sympantos` |
| Reference spec | iota-guides/08-memory.md v2.1 |
| Storage layer | SQLite `~/.i6/context/memory.sqlite` (Rust `memory.rs`) |
| Result logs | `gefsi/logs/exp01-final-*`, `gefsi/logs/exp01-full-log-*-fixed.*` |

---

## 1. Experiment goal

Validate the core claim of the iota-sympantos Memory system in multi-backend switching scenarios:

> The Engine layer is responsible for Extract / Store / Recall / Inject. Backends are replaceable; memories must not be lost.

Acceptance criteria:

| # | Criterion |
|---|-----------|
| 1 | Memories written by backend A can be recalled and injected into context by backend B |
| 2 | All six memory bucket types can be stored and injected correctly |
| 3 | contentHash (SHA-256) deduplication works |
| 4 | confidence + scope filtering is effective |
| 5 | `memory_chars: 2000` truncation behavior is observable |
| 6 | logging / tracing / metrics can be verified via observability commands |
| 7 | The full memory log chain is observable, including backend tool calls, Memory API routing, and engine recall/inject/episodic |
| 8 | Console trace outputs structured logs for memory read/write directly |

---

## 2. Experiment environment

### 2.1 Prerequisites

| Component | Result |
|-----------|--------|
| iota binary | `cargo build --release` succeeded |
| nimia.yaml | `C:\Users\feuye\.i6\nimia.yaml` |
| SQLite CLI | `C:\Users\feuye\Tools\sqlite\sqlite3.exe` |
| SQLite version | `3.53.1 2026-05-05 ... (64-bit)` |
| SQLite compile options | `ENABLE_FTS3`, `ENABLE_FTS4`, `ENABLE_FTS5` |
| Default PATH sqlite3 | `3.44.3 ... (32-bit)`, no FTS5 support, not used for this experiment's DB operations |

Backend configuration check:

```powershell
.\target\release\iota.exe check
```

| Backend | Status | Model |
|---------|--------|-------|
| claude-code | configured | `MiniMax-M2.7` |
| codex | configured | `gh/gpt-5.4` |
| gemini | configured | `gemini-2.5-flash` |
| hermes | configured | `MiniMax-M2.7` |
| opencode | configured | `minimax-cn-coding-plan/MiniMax-M2.7` |

### 2.2 Path conventions

| Path | Purpose |
|------|---------|
| `~/.i6/nimia.yaml` | Sole configuration source |
| `~/.i6/context/memory.sqlite` | Memory storage (table `memory`) |
| `~/.i6/context/events.sqlite` | Event persistence |
| `gefsi/logs/` | Command output logs for this experiment |

### 2.3 scope_id conventions

| Scope | scope_id on write | Recall candidate range |
|-------|-------------------|------------------------|
| user | `local-user` | `[supplied value, "user-sympantos", "local-user"]` |
| project | `iota-sympantos` | `[supplied value, "iota-sympantos", cwd basename]` |
| session | auto-generated | current `session_id` |

### 2.4 Confidence filter thresholds

| Bucket | min_confidence |
|--------|----------------|
| identity | 0.85 |
| preference | 0.80 |
| strategic | 0.80 |
| domain | 0.80 |
| procedural | 0.75 |
| episodic | 0.70 |

---

## 3. Fixes applied

This round applied fixes before re-running. Summary of changes:

| File | Fix |
|------|-----|
| `src/runtime_event.rs` | Normalize `mcp__iota-context__iota_memory_*` events from ACP `tool_call_update` into real `ToolCall` / `ToolResult`; prevent intermediate empty `content: []` from being misclassified as a result |
| `src/cli/mod.rs` | Add `[memory:read]`, `[memory:read:result]`, `[memory:write]`, `[memory:write:result]` console logs under `--trace` |
| `src/mcp/client.rs` | Forward sidecar stderr from the iota internal MCP client to the console, enabling local route diagnostics |
| `src/runtime_event_tests.rs` | Add normalization tests for `tool_call_update` write, read, and failure results |

Key observations after applying fixes:

```text
[memory:write] id=call_function_xigjqebwv2ab_1 type=semantic facet=identity scope=user scope_id=local-user confidence=0.5 content_chars=16 args=...
[memory:write:result] id=call_function_xigjqebwv2ab_1 ok=true memory_id=a2528017-a4f9-4e04-8a07-a26a64b23c11 value=...
[memory:read] id=call_function_8hglqvifb4la_1 query=exp01-full-log-probe-20260507-fixed limit=5 mode=hybrid args=...
[memory:read:result] id=call_function_8hglqvifb4la_1 ok=true record_count=5 value=...
```

Additional fixes applied (same day):

| Suggestion | Implementation | Verification |
|------------|----------------|--------------|
| Backend tool write schema / client validation | `iota_memory_write` MCP schema and runtime entry both require `content`, `type`, `scope`, `confidence`; `confidence` must be in `[0,1]`; schema and runtime both enforce that `semantic` requires `facet`, and `episodic`/`procedural` must not include `facet` | `cargo test memory_write` passes; direct `context-mcp` call missing confidence returns `isError=true`, `confidence is required` |
| Backend-managed sidecar route logs forwarded to main log | Default `iota-context` MCP server injects `RUST_LOG=iota::context::server=info`; ACP backend stderr forwards memory-route-related lines in non-`--show-native` mode | `cargo test context_mcp_server_enables_memory_route_logging` passes |
| `observability logging tools` filter by tool name and call/result audit | Added `--tool NAME` / `--tool-name NAME`; added `--mode calls\|results\|pairs` where `pairs` outputs call/result paired audit view grouped by execution and tool call id | `cargo run -- observability logging tools --limit 3 --tool iota_memory_write --mode pairs` returns only closed-loop `iota_memory_write` records |

2026-05-08 — full structured log output implementation:

| Item | Implementation | Expected verification |
|------|----------------|----------------------|
| Unified structured log event | Added `RuntimeEvent::Log(LogEvent)` with fields: `ts`, `level`, `target`, `execution_id`, `session_id`, `backend`, `route`, `event`, `tool_name`, `tool_call_id`, `ok`, `latency_ms`, `fields` | `observability logging events <execution-id>` shows `event_type=log` |
| Console trace shared structure | `iota run --trace` `[memory:read/write]` output rendered from `LogEvent`, preventing drift from EventStore fields | Memory tool call/result console format remains compatible |
| Engine memory structured audit | recall started/completed/failed, inject, engine-keyword write, episodic write, compaction all written to `RuntimeEvent::Log` | `observability logging logs --event memory.write.result` can query engine write results |
| MCP sidecar route JSONL | `context-mcp` emits `[iota log] {...LogEvent...}` on memory search/write call/result; failures also emit `ok=false` | Direct `context-mcp` or backend stderr forwarding shows structured route lines |
| Tools audit scan completeness | `observability logging tools` adds `--scan N`, defaults to scanning at least 500 executions | Sparse tool calls no longer affected by `limit * 5` sampling |
| Pairs anomaly status | `--mode pairs` adds `status=completed/missing_call/missing_result`; `result_seq` and `ok` may be null | Diagnose chains with only a call or only a result |
| Log query entry point | Added `observability logging logs [--event NAME] [--scan N]` | Filter logs directly by structured event name |

---

## 4. Experiment steps and results

### Step 0 - Environment setup

Execute:

```powershell
cargo build --release
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" --version
```

Result:

| Check | Result |
|-------|--------|
| release build | pass |
| sqlite version | `3.53.1 2026-05-05 ... (64-bit)` |
| FTS5 | pass |

Clean test scope data:

```powershell
$sqlite = "$env:USERPROFILE\Tools\sqlite\sqlite3.exe"
$memoryDb = "$env:USERPROFILE\.i6\context\memory.sqlite"
& $sqlite $memoryDb "DELETE FROM memory
  WHERE scope_id IN ('user-sympantos','iota-sympantos','local-user')
     OR scope_id LIKE '%iota-sympantos'
     OR content LIKE '%exp01-full-log-probe-%'
     OR content LIKE '%domain-padding-%'
     OR content LIKE '%低置信度测试%';"
```

Result: matching record count after cleanup is `0`.

Note: the Android `sqlite3.exe` on PATH does not support FTS5 and will fail when the delete triggers the `memory_fts` trigger; this experiment uses `C:\Users\feuye\Tools\sqlite\sqlite3.exe` instead.

---

### Step 1 - Write 6 memory types via MCP tool (claude-code)

Execution: `claude-code` backend calls `iota_memory_write` via the `iota-context` MCP tool. Each command uses `--trace --timeout-ms 180000` and writes output to `gefsi/logs/exp01-final-step1-*.txt`.

DB results this run:

| Sub-step | type | facet | scope | scope_id | confidence | short_id | Result |
|----------|------|-------|-------|----------|------------|----------|--------|
| 1-A | semantic | identity | user | local-user | 0.95 | `a68ec01a` | pass |
| 1-B | semantic | preference | user | local-user | 0.90 | `84ec24a4` | pass |
| 1-C | semantic | strategic | project | iota-sympantos | 0.90 | `3b0e6dad` | pass |
| 1-D | semantic | domain | project | iota-sympantos | 0.90 | `680aeb70` | pass |
| 1-E | procedural | - | project | iota-sympantos | 1.00 | `ac413811` | pass; backend omitted confidence, storage layer used default 1.00 |
| 1-F | episodic | - | project | iota-sympantos | 0.80 | `d75d5464` | pass |

Total: 6 records, 1 per bucket.

---

### Step 2 - identity recall verification (codex)

Execute:

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "我是谁？请介绍你对我的了解"
```

Result:

| Check | Result |
|-------|--------|
| Backend reply | contains `用户名 Sympantos`, `iota-sympantos 实验员`, `跨后端记忆延续验证` |
| trace | `[memory:inject]` `identity` contains `a68ec01a` |
| Verdict | pass |

---

### Step 3 - preference recall verification (gemini)

Execute:

```powershell
.\target\release\iota.exe run --backend gemini --trace --timeout-ms 180000 "你知道我的回答语言偏好和报告格式吗？"
```

Result:

| Check | Result |
|-------|--------|
| Backend reply | Chinese reply mentioning Chinese answers, English logs/commands/code identifiers, Markdown, 2-space indent |
| trace | `[memory:inject]` `preference` contains `84ec24a4` |
| Verdict | pass |

---

### Step 4 - strategic + domain recall verification (hermes)

Execute:

```powershell
.\target\release\iota.exe run --backend hermes --trace --timeout-ms 180000 "告诉我当前项目的目标和技术实现"
```

Result:

| Check | Result |
|-------|--------|
| Backend reply | mentions 2026 Q2 goal, Rust, SQLite, recall/inject, SHA-256 content_hash, 6 buckets |
| trace | `strategic` contains `3b0e6dad`, `domain` contains `680aeb70` |
| Verdict | pass |

---

### Step 5 - procedural + episodic recall verification (opencode)

Execute:

```powershell
.\target\release\iota.exe run --backend opencode --trace --timeout-ms 180000 "回顾实验步骤，以及本次实验发生了什么"
```

Result:

| Check | Result |
|-------|--------|
| Backend reply | covers 6-step experiment flow and Step 1 writing 6 memory types |
| trace | `procedural` contains `ac413811`, `episodic` contains `d75d5464` |
| Verdict | pass |

---

### Step 6 - contentHash deduplication verification

Repeat write of the Step 1-A identity content.

Before write:

| id | hash12 | created_at | updated_at | confidence |
|----|--------|------------|------------|------------|
| `a68ec01a-0d3a-44f9-a859-ad4aeab93722` | `5ee43f7ae37d` | `1778155663` | `1778155663` | 0.95 |

After duplicate write:

| id | hash12 | created_at | updated_at | confidence |
|----|--------|------------|------------|------------|
| `a68ec01a-0d3a-44f9-a859-ad4aeab93722` | `5ee43f7ae37d` | `1778155663` | `1778155776` | 0.95 |

Result:

| Check | Result |
|-------|--------|
| Row count for same content | `1` |
| ID/hash | unchanged |
| `updated_at` | updated |
| Console log | `[memory:write]` and `[memory:write:result]` show real memory_id `a68ec01a...` |
| Verdict | pass |

---

### Step 7 - Confidence filter verification

Goal: write identity and procedural records below threshold; verify they exist in DB but are not injected.

This run:

| Record | Write method | short_id | confidence | Result |
|--------|-------------|----------|------------|--------|
| low identity | claude-code tool call | `a2528017` | 0.50 | below 0.85, not injected |
| low procedural | direct `iota context-mcp` JSON-RPC | `c74b1f49` | 0.60 | below 0.75, not injected |

Verification command:

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "你知道关于我的所有信息吗？"
```

Result:

| Check | Result |
|-------|--------|
| DB | `a2528017` and `c74b1f49` both present |
| trace identity | contains only high-confidence `a68ec01a`, not the low-confidence test text |
| trace procedural | contains only high-confidence `ac413811`, not the low-confidence test text |
| Console log | low identity write shows `[memory:write] confidence=0.5` and `[memory:write:result] memory_id=a2528017...` |
| Verdict | pass |

---

### Step 8 - Token budget truncation verification

Preparation: write 15 `domain-padding-N` records via `iota context-mcp`, confidence=0.90.

Statistics:

| Metric | Value |
|--------|-------|
| padding_count | 15 |
| padding_chars | 2481 |
| eligible_chars | 2916 |

Trigger recall:

```powershell
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "列出你知道的关于我和本项目的所有信息"
```

Budget from trace:

```json
{"memory_chars":2000,"total_chars":2916,"truncated":true,"excluded_count":7}
```

Verdict: pass.

---

### Step 9 - Observability audit

Execute:

```powershell
.\target\release\iota.exe observability logging recent --limit 80
.\target\release\iota.exe observability tracing summary
.\target\release\iota.exe observability metrics
.\target\release\iota.exe observability metrics --prometheus
.\target\release\iota.exe observability logging events 6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e
.\target\release\iota.exe observability tracing breakdown 6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e
.\target\release\iota.exe observability logging tools --limit 20
.\target\release\iota.exe observability logging tools --limit 3 --tool iota_memory_write --mode pairs
```

Local EventStore aggregate statistics (includes historical runs, not limited to this experiment):

| Metric | Value |
|------|----|
| total_executions | 102 |
| completed_executions | 93 |
| failed_executions | 4 |
| running_executions | 5 |
| avg_prompt_ms | 11293.59 |
| avg_total_ms | 12398.13 |
| p95_total_ms | 24780 |
| cache.hit_rate | 0.07258064516129033 |
| token usage events | 0 |

This round Steps 1–8 cover fencing tokens `89..102`, spanning `claude-code`, `codex`, `gemini`, `hermes`, `opencode`.

Step 7 low identity breakdown:

| phase | ms |
|-------|----|
| process_spawn | 13 |
| init | 1182 |
| session_new | 785 |
| prompt | 15300 |
| total | 16085 |

EventStore event stream check:

| execution_id | Result |
|--------------|--------|
| `6b8a00be-6ff4-4653-92a6-5e0f1a51ce3e` | contains `state started`, `memory inject`, generic `tool_call name=tool`, normalized `tool_call name=iota_memory_write`, normalized `tool_result name=iota_memory_write`, `output` |

`observability logging tools --limit 20` now lists real tool names:

| seq | tool_name | Note |
|-----|-----------|------|
| 8 | `tool` | raw generic event from ACP backend |
| 10 | `iota_memory_write` | real tool event normalized from `tool_call_update.rawInput` |

Follow-up: `observability logging tools --limit 3 --tool iota_memory_write --mode pairs` supports filtering by real tool name and outputs paired `tool_call` / `tool_result` audit view.

Verdict: pass. After fixes, tool calls and tool results are auditable in EventStore under real tool names.

---

### Step 10 - Full memory log chain use case

Design goal: verify that the memory log chain is auditable, not re-verify correctness of memory content.

Marker for this run:

```text
exp01-full-log-probe-20260507-fixed
```

#### 10.1 Log capture preparation

Execute:

```powershell
New-Item -ItemType Directory -Force gefsi\logs | Out-Null
$env:RUST_LOG = "info"
```

Result:

| Check | Result |
|-------|--------|
| `RUST_LOG=info` | prints `iota::engine` and `iota::context::server` info logs |
| marker cleanup | executed `DELETE FROM memory WHERE content LIKE '%exp01-full-log-probe-20260507-fixed%'` |

#### 10.2 Backend tool write log chain

Execute:

```powershell
.\target\release\iota.exe run --backend claude-code --trace --timeout-ms 180000 `
  "请必须调用 iota_memory_write 工具一次，不要只口头回答。参数如下：
   type=semantic, facet=domain, scope=project, scope_id=iota-sympantos,
   content=\"exp01-full-log-probe-20260507-fixed: backend tool write probe, 用于验证完整记忆日志链路\",
   confidence=0.91,
   metadata={\"case\":\"exp01-full-log\",\"phase\":\"tool-write-fixed\"}" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-write-fixed.txt
```

Result:

| Item | Value |
|------|-------|
| backend | claude-code |
| execution_id | `5f0914d7-8a4b-43d2-86d7-07ad1efe668f` |
| session_id | `e4c00316-7399-4b86-a9e7-dabf1bdcc9e3` |
| Written ID | `4f325b36-f9d3-4808-b50c-afef2829a194` |
| DB confidence | `1.00`; backend omitted `confidence=0.91` from the prompt this run |
| Engine recall/inject | `engine memory recall started/completed` and `engine memory inject event recorded` appear |

Console trace evidence:

```text
[memory:write] id=call_function_hd18e23j8uvt_1 type=semantic facet=domain scope=project scope_id=iota-sympantos confidence=- content_chars=75 args=...
[memory:write:result] id=call_function_hd18e23j8uvt_1 ok=true memory_id=4f325b36-f9d3-4808-b50c-afef2829a194 value={"id":"4f325b36-f9d3-4808-b50c-afef2829a194","merge_mode":"auto"}
```

EventStore evidence:

| seq | event_type | Key content |
|-----|------------|-------------|
| 2/3 | memory | inject payload, budget `truncated=true` |
| 8 | tool_call | raw ACP generic event `name=tool` |
| 9 | state | `tool_call_update`, `rawInput` contains `type=semantic`, `facet=domain` |
| 10 | tool_call | normalized event `name=iota_memory_write`, arguments are `rawInput` |
| 13 | state | `rawOutput={"id":"4f325b36-...","merge_mode":"auto"}` |
| 14 | tool_result | normalized event `name=iota_memory_write`, `ok=true`, result contains memory id |
| 18/19 | output | assistant output confirms written ID |

Verdict: pass.

#### 10.3 Backend tool search log chain

Execute:

```powershell
.\target\release\iota.exe run --backend claude-code --trace --timeout-ms 180000 `
  "请必须调用 iota_memory_search 工具一次，不要只口头回答。参数如下：
   query=\"exp01-full-log-probe-20260507-fixed\", limit=5, mode=hybrid。
   然后用一句话总结搜索结果。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-search-fixed.txt
```

Result:

| Item | Value |
|------|-------|
| backend | claude-code |
| execution_id | `7445ea10-52cf-4fee-a82e-0ab8c5a5235a` |
| session_id | `4f3cda7f-5f5b-4320-9006-62a1739e5615` |
| Search results | 5 records |
| Key record hit | `4f325b36`, content contains `exp01-full-log-probe-20260507-fixed` |
| Engine recall/inject | `engine memory recall started/completed` and `engine memory inject event recorded` appear |
| Auto episodic | episodic write `db08c47e-feb0-4382-a2ad-a87f3ee74957` after search turn |

Console trace evidence:

```text
[memory:read] id=call_function_8hglqvifb4la_1 query=exp01-full-log-probe-20260507-fixed limit=5 mode=hybrid args=...
[memory:read:result] id=call_function_8hglqvifb4la_1 ok=true record_count=5 value=...
```

EventStore evidence:

| seq | event_type | Key content |
|-----|------------|-------------|
| 8 | tool_call | raw ACP generic event `name=tool` |
| 9 | state | `tool_call_update`, `rawInput.query=exp01-full-log-probe-20260507-fixed` |
| 10 | tool_call | normalized event `name=iota_memory_search` |
| 13 | state | `rawOutput` contains `mode=hybrid`, `records`, `4f325b36` |
| 14 | tool_result | normalized event `name=iota_memory_search`, `ok=true`, `records` count is 5 |
| 19/20 | output | assistant summarizes search results |

Verdict: pass.

#### 10.4 Engine auto-episodic write log chain

Execute:

```powershell
.\target\release\iota.exe run --backend gemini --trace --timeout-ms 180000 `
  "请用一句话回答：exp01-full-log-probe-20260507-fixed 普通 turn 日志探针已收到。" `
  *>&1 | Tee-Object gefsi\logs\exp01-full-log-episodic-fixed.txt
```

Result:

| Item | Value |
|------|-------|
| backend | gemini |
| execution_id | `ab4afaad-946a-408a-b1e6-8b6cb8504306` |
| session_id | `cbe42e54-52a4-47e6-9e4b-660aa3b22101` |
| output | `好的，exp01-full-log-probe-20260507-fixed 普通 turn 日志探针已收到。` |
| episodic memory_id | `b14be7f7-b680-464c-8ffc-97a9a87c375c` |

Log evidence:

```text
engine memory recall started
engine memory recall completed
engine memory inject event recorded
engine episodic memory write started
engine episodic memory write completed memory_id=b14be7f7-b680-464c-8ffc-97a9a87c375c
engine episodic memory compaction completed
[memory:inject]
```

Verdict: pass.

#### 10.5 Memory API route log chain

The `context-mcp` stdio server injected via `session/new` is managed by the backend process. A subsequent fix injects `RUST_LOG=iota::context::server=info` into the default `iota-context` sidecar, and forwards memory-route-related lines from ACP backend stderr in non-`--show-native` mode; if the backend passes sidecar stderr back to the ACP process stderr, the `iota run` main log can capture those route lines. To directly verify the Memory API route, this run still retains a direct sidecar probe:

```powershell
$env:RUST_LOG = "info"
@($init, $ready, $call) | .\target\release\iota.exe context-mcp *>&1 |
  Tee-Object gefsi\logs\exp01-full-log-route-direct-fixed.txt
```

Result:

| Log fragment | Result |
|--------------|--------|
| `context MCP memory search tool call received` | present, `query=exp01-full-log-probe-20260507-fixed`, `limit=5`, `mode=Hybrid` |
| `context MCP memory search tool call completed` | present, `record_count=5` |
| `record_ids` | contains `4f325b36`, `b14be7f7`, `db08c47e` |

Verdict: pass. Memory API route is self-observable; the main process already has selective stderr forwarding; remaining gap depends on whether the specific backend passes sidecar stderr back.

#### 10.6 Automated log file check

Files checked:

| File | Key result |
|------|------------|
| `exp01-full-log-write-fixed.txt` | engine recall/inject, `[memory:inject]`, `[memory:write]`, `[memory:write:result]` |
| `exp01-full-log-search-fixed.txt` | engine recall/inject, `[memory:inject]`, `[memory:read]`, `[memory:read:result]`, search summary |
| `exp01-full-log-episodic-fixed.txt` | engine recall/inject, auto-episodic started/completed/compaction |
| `exp01-full-log-route-direct-fixed.txt` | memory API route received/completed, `record_count=5` |
| `exp01-full-log-events-write-fixed.json` | `tool_call name=iota_memory_write` and `tool_result name=iota_memory_write` |
| `exp01-full-log-events-search-fixed.json` | `tool_call name=iota_memory_search` and `tool_result name=iota_memory_search` |

Verdict: pass.

#### 10.7 EventStore persistence verification

Most recent three successful probes:

| execution_id | backend | status | Key evidence |
|--------------|---------|--------|--------------|
| `5f0914d7-8a4b-43d2-86d7-07ad1efe668f` | claude-code | completed | `tool_call/tool_result iota_memory_write` |
| `7445ea10-52cf-4fee-a82e-0ab8c5a5235a` | claude-code | completed | `tool_call/tool_result iota_memory_search` |
| `ab4afaad-946a-408a-b1e6-8b6cb8504306` | gemini | completed | engine auto-episodic |

EventStore conclusions:

| Evidence | Result |
|----------|--------|
| `state started` | present |
| `memory inject` | present |
| `tool_call` | present; includes both raw `tool` and normalized real tool name |
| `tool_result` | present; includes `iota_memory_write` / `iota_memory_search` |
| `output` | present |

Verdict: pass.

#### 10.8 DB-side confirmation

Query:

```powershell
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" -header -column `
  "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "SELECT substr(id,1,8) AS short_id, type, facet, scope, scope_id, confidence, substr(content,1,120)
   FROM memory
   WHERE content LIKE '%exp01-full-log-probe-20260507-fixed%'
   ORDER BY updated_at DESC;"
```

Result:

| short_id | type | facet | scope | scope_id | confidence | Note |
|----------|------|-------|-------|----------|------------|------|
| `b14be7f7` | episodic | - | session | `cbe42e54-52a4-47e6-9e4b-660aa3b22101` | 0.80 | gemini plain-turn auto-episodic |
| `db08c47e` | episodic | - | session | `4f3cda7f-5f5b-4320-9006-62a1739e5615` | 0.80 | claude-code search-turn auto-episodic |
| `4f325b36` | semantic | domain | project | `iota-sympantos` | 1.00 | backend tool write probe |

Verdict: pass.

#### 10.9 Cleanup state

Probe records were not deleted before completing this document, because the DB-side evidence for Step 10 needed to remain until the document was finished. Run the following when cleanup is needed:

```powershell
& "$env:USERPROFILE\Tools\sqlite\sqlite3.exe" "$env:USERPROFILE\.i6\context\memory.sqlite" `
  "DELETE FROM memory
   WHERE scope_id IN ('local-user','iota-sympantos')
      OR scope_id LIKE '%iota-sympantos'
      OR content LIKE '%exp01-full-log-probe-%'
      OR content LIKE '%domain-padding-%'
      OR content LIKE '%低置信度测试%';"
```

#### 10.10 Structured log output additional verification (2026-05-08)

Additional verification after implementing unified `LogEvent` this round:

```powershell
cargo test
cargo build --release
.\target\release\iota.exe run --backend codex --trace --timeout-ms 180000 "我叫 exp-log-event-20260508"
.\target\release\iota.exe observability logging logs --limit 5 --event memory.write.result --scan 50
.\target\release\iota.exe observability logging tools --limit 5 --tool iota_memory_write --mode pairs --scan 500
@($init, $ready, $call) | .\target\release\iota.exe context-mcp *>&1
```

Result:

| Check | Result |
|-------|--------|
| `cargo test` | pass, 107 passed |
| `cargo build --release` | pass |
| Console trace | memory-write-only turn outputs `[memory:write] {...LogEvent...}`, event is `memory.write` |
| `observability logging logs` | returns `event_type=log`, `event=memory.write.result`, `backend=codex`, `route=engine`, `ok=true` |
| `observability logging tools --mode pairs --scan 500` | returns `status=completed`, `call_seq`, `result_seq`, `ok=true`, filtered to `iota_memory_write` |
| `context-mcp` route JSONL | stderr outputs `[iota log] {...}` containing `memory.write.call` and `memory.write.result`, `route=mcp-sidecar` |
| Test data cleanup | deleted `exp-log-event-20260508` and `exp-route-log-20260508` memory records |

New command behaviors:

| Command | Description |
|---------|-------------|
| `observability logging logs [--event NAME] [--scan N]` | query persisted structured `LogEvent` |
| `observability logging tools --mode pairs` | pair output adds `status=completed/missing_call/missing_result` |
| `observability logging tools --scan N` | control execution scan window, defaults to at least 500 |

---

## 5. Acceptance matrix

| # | Criterion | Step | Result |
|---|-----------|------|--------|
| 1 | identity cross-backend continuity | Step 2 | pass |
| 2 | preference cross-backend continuity | Step 3 | pass |
| 3 | strategic + domain cross-backend continuity | Step 4 | pass |
| 4 | procedural + episodic continuity | Step 5 | pass |
| 5 | contentHash deduplication | Step 6 | pass |
| 6 | confidence filter (identity) | Step 7 | pass |
| 7 | confidence filter (procedural) | Step 7 | pass |
| 8 | token budget truncation | Step 8 | pass, `truncated=true`, `excluded_count=7` |
| 9 | SQLite schema compliance | Step 1 | pass |
| 10 | trace event completeness | Step 2–5 | pass |
| 11 | EventStore persistence | Step 9 | pass |
| 12 | logging multi-backend coverage | Step 9 | pass |
| 13 | tracing latency breakdown | Step 9 | pass |
| 14 | metrics queryable | Step 9 | pass |
| 15 | Prometheus export | Step 9 | pass |
| 16 | Step 10 engine recall/inject log | Step 10 | pass |
| 17 | Step 10 backend MCP tool call audit | Step 10 | pass; real `iota_memory_*` tool names normalized |
| 18 | Step 10 memory API route log | Step 10 | pass; direct `context-mcp` shows received/completed/record_count |
| 19 | Step 10 engine auto-episodic | Step 10 | pass |
| 20 | Console memory read/write log | Step 6/7/10 | pass; new `[memory:read/write]` and result logs present |

Conclusion: core memory continuity, deduplication, filtering, budget, observability, and full log chain all pass. This round fixed the RuntimeEvent tool name/result normalization issues exposed in the previous run and added structured console memory read/write logs.

---
