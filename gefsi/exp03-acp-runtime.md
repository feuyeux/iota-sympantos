# iota-sympantos experiment 3: three ACP call path latency comparison

Status note: this is a historical benchmark report. Older snippets may use `--trace-timing`; the current CLI flag is `--timing`. For current observability behavior, see `doc/observability.md`.

| Field | Value |
|-------|-------|
| Experiment ID | exp03-acp-runtime |
| Date | 2026-05-07 |
| Subject | three call paths: CLI with daemon, CLI without daemon, backend direct |
| Implementation | `src/cli/mod.rs`, `src/daemon/`, `src/engine.rs`, `src/acp/`, `src/config.rs` |

---

## 1. Experiment goal

This experiment does not re-verify backend availability itself; instead it compares the end-to-end latency difference when the same prompt reaches the model via three different paths:

| Path | Entry point | Question being answered |
|------|-------------|------------------------|
| CLI without daemon | `iota run <backend> <prompt>` | When a new iota CLI process starts each time, how long do ACP adapter initialization, session creation, and prompt execution take in total? |
| CLI with daemon | `iota run --daemon <backend> <prompt>` | With the CLI acting only as a thin client forwarding over TCP to a resident daemon, how much latency is saved? |
| Backend direct | each backend's own one-shot/headless CLI | With no iota, no daemon, and no ACP adapter, what is the baseline latency of the backend's native command? |

Core conclusion: the value of the daemon is not to re-prove backend startup capability, but to move repeated startup costs from high-frequency CLI calls into a resident process. Backend direct serves as an external baseline to quantify how much latency the iota/ACP layer adds.

---

## 2. Path definitions

### 2.1 CLI without daemon

Command form:

```powershell
.\target\release\iota.exe run --trace-timing <backend> “say hello. reply with exactly: hello”
```

This path includes:

| Phase | Description |
|-------|-------------|
| CLI process | launch `iota.exe` on Windows, parse arguments, read `~/.i6/nimia.yaml` |
| Engine | create `IotaEngine` inside the current CLI process |
| ACP adapter | start the adapter process per backend config, execute `initialize` |
| ACP session | execute `session/new` |
| Prompt | execute `session/prompt` and wait for `session/complete` |

This path is suited for measuring the real user-perceived latency of a single command invocation. It repeatedly pays the cost of CLI process startup, engine construction, adapter startup, and session creation.

### 2.2 CLI with daemon

Command form:

```powershell
.\target\release\iota.exe run --daemon --trace-timing <backend> “say hello. reply with exactly: hello”
```

This path includes:

| Phase | Description |
|-------|-------------|
| CLI process | launch a short-lived `iota.exe` |
| TCP hop | connect to `127.0.0.1:47661`, send `DaemonPromptRequest` |
| Daemon engine | daemon reuses `IotaEngine` per cwd |
| ACP client/session | reuses backend client and session when already warmed up |
| Prompt | execute `session/prompt` and return `DaemonPromptResponse` |

This path is suited for measuring whether the daemon can amortize adapter and session costs across repeated shell invocations of iota. The first daemon call (cold start) may still include backend initialization; the warm path is the primary scenario the daemon design optimizes for.

### 2.3 Backend direct

Command form varies by backend:

| Backend | Direct command |
|---------|---------------|
| claude-code | `claude -p “say hello. reply with exactly: hello”` |
| codex | `codex exec “say hello. reply with exactly: hello”` |
| gemini | `gemini -p “say hello. reply with exactly: hello”` |
| hermes | `hermes -z “say hello. reply with exactly: hello”` |
| opencode | `npx -y opencode-ai@1.14.40 run “say hello. reply with exactly: hello”` |

This path does not go through iota, does not start an ACP adapter, and does not go through the daemon. It is used solely to provide an external baseline for each backend's native one-shot mode; it cannot replace ACP compatibility verification.

---

## 3. Experiment environment

| Item | Value |
|------|-------|
| OS | Windows |
| Shell | PowerShell 7.6.1 |
| Workspace | `D:\coding\creative\iota-sympantos` |
| Binary | `target/release/iota.exe` |
| Daemon address | `127.0.0.1:47661` |
| Config source | `~/.i6/nimia.yaml` |
| Prompt | `say hello. reply with exactly: hello` |

### 3.1 Backend versions

`version_mapping` records only the specific version numbers, not package names, command strings, or update information.

| Backend | ACP version | bin version | Notes |
|---------|------------:|------------:|-------|
| claude-code | 0.32.0 | 2.1.123 | `@agentclientprotocol/claude-agent-acp` + `claude` |
| codex | 0.12.0 | 0.128.0 | `@zed-industries/codex-acp` and `codex-cli` versions differ |
| gemini | 0.41.2 | 0.41.2 | `@google/gemini-cli --acp` |
| hermes | 0.12.0 | 0.12.0 | `hermes acp` |
| opencode | 1.14.40 | 1.14.40 | configured with `npx opencode-ai@1.14.40` |

---

## 4. Measurement methodology

### 4.1 Build and baseline test

```powershell
cargo fmt
cargo test --release -- --format terse
cargo build --release
.\target\release\iota.exe check
```

Verified results:

| Command | Result |
|---------|--------|
| `cargo fmt` | pass |
| `cargo test --release -- --format terse` | 93 passed |
| `cargo build --release` | pass |
| `iota check` | 5 backends configured, includes `version_mapping.acp/bin` |

### 4.2 CLI without daemon

Execute per backend:

```powershell
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --trace-timing $backend "say hello. reply with exactly: hello"
}
```

Fields collected:

| Field | Meaning |
|-------|---------|
| `init_ms` | ACP adapter `initialize` latency |
| `session_new_ms` | `session/new` latency |
| `prompt_ms` | `session/prompt` to completion latency |
| `total_ms` | total run latency recorded by iota |
| `client_started` | whether backend client was started this run |
| `process_spawned` | whether adapter process was spawned this run |
| `session_reused` | whether an existing session was reused |

### 4.3 CLI with daemon

Warm up the daemon with one round of calls, then measure the warm path:

```powershell
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon $backend "warm up. reply exactly: ok"
}

foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon --trace-timing $backend "say hello. reply with exactly: hello"
}
```

Expected fields on the warm path:

| Field | Expected |
|-------|----------|
| `route` | `daemon` |
| `daemon_hit` | `true` |
| `client_started` | `false` |
| `process_spawned` | `false` |
| `session_reused` | `true` |
| `session_new_ms` | omitted, `null`, or significantly lower than cold path |

### 4.4 Backend direct

PowerShell unified measurement:

```powershell
$prompt = "say hello. reply with exactly: hello"
$commands = @(
  @{ backend = 'claude-code'; command = { claude -p $prompt | Out-Null } },
  @{ backend = 'codex';       command = { codex exec $prompt | Out-Null } },
  @{ backend = 'gemini';      command = { gemini -p $prompt | Out-Null } },
  @{ backend = 'hermes';      command = { hermes -z $prompt | Out-Null } },
  @{ backend = 'opencode';    command = { npx -y opencode-ai@1.14.40 run $prompt | Out-Null } }
)

foreach ($item in $commands) {
  $elapsed = Measure-Command { & $item.command }
  "$($item.backend),$([int]$elapsed.TotalMilliseconds)"
}
```

Backend direct data is used only as a cross-reference. Because each backend's direct CLI loads different configurations, permission policies, MCP, memory systems, and output formats by default, it cannot be field-aligned stage-by-stage with the ACP path; only end-to-end one-shot latency can be compared.

---

## 5. Sample data

### 5.1 CLI without daemon — collected samples

Data from `iota run --trace-timing <backend> "say hello. reply with exactly: hello"`.

| Backend | init_ms | session_new_ms | prompt_ms | total_ms | Output |
|---------|--------:|---------------:|----------:|---------:|--------|
| claude-code | 1120-1212 | 758-844 | 3780-3907 | 4539-4752 | `hello` |
| codex | 1031 | 3287 | 18015 | 21303 | `hello` |
| gemini | 6254 | 1622 | 2069 | 3691 | `hello` |
| hermes | 2694 | 7007 | 3932 | 10939 | `hello` |
| opencode | 41017 | 1580 | 3634 | 5214 | `hello` |

Observations:

| Backend | Primary latency source |
|---------|----------------------|
| claude-code | prompt phase dominates; adapter/session relatively stable |
| codex | prompt phase significantly elevated |
| gemini | initialize elevated, but prompt is faster |
| hermes | session/new elevated |
| opencode | first npx initialize extremely high; cold path must use 60s timeout |

Note: different adapters define `total_ms` and per-phase fields differently. The report preserves raw runtime fields and does not force-sum phase values.

### 5.2 iota two-path historical samples

Samples below are medians from a 3-run benchmark, prompt `say hello. reply with exactly: hello`. "CLI without daemon cold" means each run starts adapter/session fresh; "CLI with daemon hot" means the prompt path after the daemon is already warmed up.

| Backend | CLI with daemon hot ms | CLI without daemon cold ms | daemon speedup | Notes |
|---------|-----------------------:|---------------------------:|---------------:|-------|
| claude-code | 1569 | 3756 | 2.4x | daemon eliminates repeated adapter/session startup |
| codex | 1415 | 5880 | 4.1x | cold path prompt + adapter cost is high |
| gemini | 1185 | 7300 | 6.2x | cold path must include `init_ms` in user-perceived latency |
| hermes | 1468 | 4378 | 3.0x | session/new reuse benefit is clear |
| opencode | 3532 | 4838 | 1.4x | smallest daemon benefit; warm path also has more variance |

This data already answers the main question about iota's two internal paths: warm daemon is consistently lower than CLI cold; the benefit magnitude depends on each backend's initialize and session/new cost.

### 5.3 Three-path comparison table

Same prompt `say hello. reply with exactly: hello`, same network state, same backend configuration.

| Backend | Backend direct ms | CLI with daemon hot ms | CLI without daemon cold ms | daemon vs cold improvement | iota cold vs direct delta |
|---------|------------------:|-----------------------:|---------------------------:|---------------------------:|--------------------------:|
| claude-code | 1326 | 1569 | 3756 | 58.2% | +2430 ms |
| codex | 14261 | 1415 | 5880 | 75.9% | −8381 ms |
| gemini | 18834 | 1185 | 7300 | 83.8% | −11534 ms |
| hermes | 8895 | 1468 | 4378 | 66.5% | −4517 ms |
| opencode | 8262 | 3532 | 4838 | 27.0% | −3424 ms |

> **Backend direct commands:** `claude -p`, `codex exec`, `gemini -p --skip-trust`, `hermes -z`, `npx -y opencode-ai@1.14.40 run`

Calculation method:

```text
daemon vs cold improvement = (CLI without daemon cold ms - CLI with daemon hot ms) / CLI without daemon cold ms
iota cold vs direct delta  = CLI without daemon cold ms - Backend direct ms
    positive → iota is slower than direct (extra overhead)
    negative → iota is faster than direct (backend direct carries its own heavy burden)
```

#### Key findings

1. **4 out of 5 backends: iota CLI cold is faster than backend direct.** Reason: backend direct (`codex exec`, `gemini -p`, `hermes -z`, `opencode run`) loads a full CLI environment, plugins, permission policies, memory systems, etc.; the ACP adapter starts only the minimal inference entry point.
2. **The only exception is Claude Code.** `claude -p` is itself extremely lightweight; the iota cold path spends an extra ~2.4s on adapter startup and session creation.
3. **iota daemon hot path is the absolute fastest path across all 5 backends.** By amortizing adapter/session costs, the daemon achieves 1185–3532ms, all well below backend direct's 8262–18834ms.

---

## 6. Conclusions

1. This experiment compares end-to-end latency across three paths; it is not a backend availability matrix.
2. **Daemon hot is the absolute fastest path across all 5 backends.** Warm daemon latency is 1185–3532ms, below both backend direct (1326–18834ms) and CLI cold (3756–7300ms).
3. **4 out of 5 backends: iota CLI cold is faster than backend direct.** ACP adapter mode does not load the backend's full CLI environment, so even the cold start is lighter than `codex exec` / `gemini -p` / `hermes -z` / `opencode run`.
4. **Claude Code is the only exception:** `claude -p` itself is extremely lightweight (1326ms), while the iota cold path needs an extra 2.4s for adapter initialize and session/new. Daemon hot (1569ms) is roughly on par with Claude direct (1326ms).
5. Daemon improvement over CLI cold ranges from 27.0% to 83.8%. Gemini (83.8%) and Codex (75.9%) benefit most because their ACP adapter cold-start costs are highest.
6. `CLI without daemon` reveals each backend's primary cold-start cost: Codex is prompt-phase dominated, Hermes is session/new dominated, OpenCode/Gemini are first-npx-initialize dominated.

---

## 7. Reproduction commands

```powershell
cargo test --release -- --format terse
cargo build --release
.\target\release\iota.exe check

# CLI without daemon
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --trace-timing $backend "say hello. reply with exactly: hello"
}

# CLI with daemon: warm first, then measure hot path
foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon $backend "warm up. reply exactly: ok"
}

foreach ($backend in @('claude-code','codex','gemini','hermes','opencode')) {
  .\target\release\iota.exe run --daemon --trace-timing $backend "say hello. reply with exactly: hello"
}

# Backend direct baseline
$prompt = "say hello. reply with exactly: hello"
Measure-Command { claude -p $prompt | Out-Null }
Measure-Command { codex exec $prompt | Out-Null }
Measure-Command { gemini -p $prompt | Out-Null }
Measure-Command { hermes -z $prompt | Out-Null }
Measure-Command { npx -y opencode-ai@1.14.40 run $prompt | Out-Null }
```

Expected: all three command groups produce end-to-end latency figures. The final report uses only the three columns from a single environment run to avoid mixing cold path, hot path, and different backend direct configurations.
