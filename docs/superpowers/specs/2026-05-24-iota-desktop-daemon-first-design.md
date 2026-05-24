# iota-desktop Daemon-first Design

## Goal

Build `iota-desktop` as a cross-platform Tauri desktop app that exposes the current iota runtime as a personal local workbench.

The MVP is **Chat-first**. The first screen centers on multi-backend agent conversation, while the GUI advantage is used to show execution transparency: streaming text, runtime events, tool calls, approval requests, timing, and token usage.

The desktop app must reuse the daemon path. If the existing daemon API is too coarse for GUI use, extend the daemon so the desktop API has the same fidelity a direct `iota-core::IotaEngine` integration would have.

## Non-Goals

- Do not build a Kanban-first product in this MVP.
- Do not replace `~/.i6/nimia.yaml` with a new desktop configuration source.
- Do not store API keys in a new project-level file.
- Do not bypass the existing `acp::permission` approval policy.
- Do not remove or break the existing CLI `iota run --daemon` request/response path.

## Product Shape

The desktop MVP is a personal local workbench for one developer.

Primary workflow:

1. Open `iota-desktop`.
2. Choose backend and model context.
3. Send a prompt from the main chat composer.
4. Watch the streamed response in the transcript.
5. Inspect execution details in a right-side inspector.
6. Approve or deny tool calls when required.
7. Review timing, token usage, tool calls, and runtime events after completion.

The chosen layout is **Chat + Right Inspector**:

- The main area remains a chat transcript with a composer.
- The top toolbar shows backend, model/provider summary, workspace/cwd, and daemon status.
- The right inspector tracks the active turn and recent turns.
- Configuration and logs are secondary navigation items, not the first screen.

## Architecture

The target dependency flow is:

```text
iota-desktop React UI
  -> Tauri commands / event bridge
  -> Desktop daemon client
  -> iota-core daemon runtime
  -> EnginePool / IotaEngine / ACP / Store
```

`iota-desktop` must not directly create or cache `IotaEngine` in the final MVP design. The Tauri layer owns desktop concerns only:

- daemon autostart and connection management
- conversion between frontend commands and daemon protocol messages
- conversion from daemon stream messages to Tauri window events
- temporary GUI state such as active turn ids and pending approval ids

`iota-core::daemon` remains the reusable local runtime boundary. It should be extended where needed so desktop gets streaming, approvals, config, backend checks, and observability without bypassing the daemon.

## Daemon Protocol

The daemon keeps the current CLI-compatible API and adds a desktop-oriented streaming API.

The existing `DaemonPromptRequest -> DaemonPromptResponse` path remains available for `iota run --daemon`.

The new desktop protocol should use typed JSON-line messages over the local TCP daemon. A minimal protocol shape:

```rust
enum DaemonClientMessage {
    Hello { client_name: String, protocol_version: u32 },
    StartTurn {
        turn_id: String,
        cwd: PathBuf,
        backend: String,
        prompt: String,
    },
    RespondApproval {
        approval_id: String,
        approved: bool,
    },
    CancelTurn {
        turn_id: String,
    },
    GetConfig,
    SaveBackendModel {
        backend: String,
        model: DesktopModelConfig,
    },
    CheckBackend {
        backend: String,
    },
    GetObservabilitySummary {
        cwd: Option<PathBuf>,
    },
}
```

```rust
enum DaemonServerMessage {
    HelloAccepted {
        protocol_version: u32,
    },
    TurnStarted {
        turn_id: String,
    },
    TextChunk {
        turn_id: String,
        chunk: String,
    },
    TurnEvent {
        turn_id: String,
        event: RuntimeEvent,
    },
    ApprovalRequested {
        turn_id: String,
        approval_id: String,
        tool_name: String,
        params: serde_json::Value,
    },
    TurnCompleted {
        turn_id: String,
        text: String,
        timing: serde_json::Value,
    },
    TurnFailed {
        turn_id: String,
        error: String,
    },
    ConfigSnapshot {
        config: DesktopConfigSnapshot,
    },
    BackendCheckResult {
        backend: String,
        ok: bool,
        details: String,
    },
    ObservabilitySummary {
        summary: serde_json::Value,
    },
}
```

The protocol names may be adjusted during implementation, but the semantic contract should stay stable: the desktop can start a turn, receive fine-grained execution updates, answer approvals, cancel work, inspect config, and check backend readiness.

## Turn Data Flow

The chat flow:

```text
Send prompt
  -> Tauri invoke start_turn(prompt, backend, cwd)
  -> daemon returns or emits turn_id
  -> daemon streams turn messages
  -> Tauri emits "turn-event" / "text-chunk" / "approval-requested"
  -> React reducer updates chat transcript and right inspector
```

The frontend should aggregate by `turn_id`, not by raw strings:

```ts
type DesktopTurn = {
  id: string;
  backend: string;
  cwd: string;
  status: "queued" | "running" | "waiting_approval" | "completed" | "failed" | "cancelled";
  userPrompt: string;
  assistantText: string;
  events: RuntimeEventView[];
  toolCalls: ToolCallView[];
  approvals: ApprovalView[];
  timing?: TimingView;
  usage?: TokenUsageView;
  error?: string;
};
```

This keeps the chat transcript and execution inspector in sync without parsing rendered text.

## UI Details

### Main Chat Area

- Backend selector includes Claude Code, Codex, Gemini CLI, Hermes, and OpenCode.
- Backend rows show configured/unconfigured and health-check state.
- The transcript streams assistant text as chunks arrive.
- The composer supports multi-line input.
- Send is disabled while a turn is actively running for the same session unless queuing is explicitly added later.

### Right Inspector

Inspector sections:

- **Turn Status**: queued, running, waiting approval, completed, failed, cancelled.
- **Timing & Usage**: start/end/duration, backend timing, token usage when available.
- **Tool Calls**: tool name, parameter summary, result summary, status.
- **Approvals**: pending approval pinned at the top with approve/deny actions.
- **Runtime Events**: high-value event list by default, raw JSON in an expanded view.

The inspector should prefer summaries and folded detail. Raw native protocol payloads are useful for debugging but should not dominate the main UX.

## Configuration

`~/.i6/nimia.yaml` remains the single configuration source.

The desktop app may provide a GUI editor for backend model fields and API keys, but it writes through daemon APIs that preserve the same config semantics as CLI/TUI.

Rules:

- API keys are masked in UI by default.
- Save operations should update only the targeted backend/model fields.
- Desktop must not introduce project-local config discovery.
- Hermes must keep its existing home behavior; do not override `HERMES_HOME`.
- Cross-platform path handling must use `PathBuf` and `dirs::home_dir()` through core helpers.

## Safety And Error Handling

- The daemon binds only to `127.0.0.1`.
- Protocol version mismatch should fail clearly and ask the user to restart or upgrade.
- If daemon is not running, Tauri should autostart it and wait for readiness.
- If a backend command or API key is missing, the selector marks it unavailable and offers configuration.
- Approval decisions still flow through `acp::permission`; desktop is only a UI for the decision.
- If the window closes or approval response is lost, the default decision is deny.
- Stream interruption marks the turn as failed or disconnected and keeps the partial event history visible.
- Raw events and native payloads should be folded to reduce accidental exposure of sensitive data.

## Implementation Phases

1. **Daemon protocol foundation**
   Add desktop protocol types, hello/version negotiation, turn ids, approval ids, and serde roundtrip tests. Preserve the existing CLI daemon request/response API.

2. **Daemon streaming turn**
   Run engine turns through `EnginePool`, bridge stream chunks and `RuntimeEvent`s into daemon messages, and route approval request/response messages.

3. **Desktop daemon client**
   Replace direct `IotaEngine` usage in `src-tauri` with a daemon client, autostart logic, stream reader, and Tauri window event bridge.

4. **Chat-first frontend state**
   Split the current `App.tsx` into focused modules, introduce a turn reducer, implement chat transcript updates, and build the right inspector.

5. **Config UX**
   Read and save config through daemon APIs. Mask secrets, show missing-key state, and support inline API key entry for a selected backend.

6. **Observability polish**
   Show token usage, timing, tool calls, approvals, and runtime event summaries in the inspector. Keep raw JSON available behind expansion.

## Testing

Required automated coverage:

- daemon protocol serde roundtrip tests
- legacy daemon request compatibility tests
- stream message framing tests
- approval respond routing tests
- `EnginePool` cwd reuse and isolation tests
- desktop Rust tests proving Tauri commands use the daemon client instead of creating `IotaEngine`
- frontend reducer tests for text chunks, runtime events, approval requests, completion, failure, and cancellation

Manual verification:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
```

Desktop manual checks:

- launch desktop dev app
- connect or autostart daemon
- run one successful prompt
- run one prompt that requires approval
- test missing API key flow
- verify timing/token/tool/runtime details appear in the inspector

## Risks

- Extending daemon streaming while preserving CLI compatibility requires careful protocol separation.
- Approval routing crosses process and UI boundaries, so lost responses must fail closed.
- If frontend state remains in one large `App.tsx`, the GUI will become difficult to test. The reducer and API client should be split early.
- Config writes can accidentally reorder or drop fields if implemented as whole-file replacement without care. The first implementation can use existing serde behavior, but the risk should be reviewed before broader config editing is added.
