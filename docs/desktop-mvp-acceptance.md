# iota-desktop MVP Acceptance Runbook

This runbook verifies the Chat-first daemon desktop baseline. Do not paste API keys, tokens, or raw secret-bearing config into logs or screenshots.

## Prerequisites

- `~/.i6/nimia.yaml` exists and is the only config source.
- At least one backend is configured with a valid API key and model.
- `iota` is available in `PATH`, or `IOTA_CLI_PATH` points to the iota CLI binary for daemon autostart.
- Desktop dependencies are installed with `npm install` in `crates/iota-desktop`.
- Existing daemon processes are acceptable; the desktop app should connect to them or autostart one.

## Automated Gates

Run from the repository root unless a command says otherwise:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cd crates/iota-desktop && npm test && npm run build
```

Expected:

- All commands exit successfully.
- `iota-cli -- check` does not print API keys or tokens.
- Frontend tests include reducer coverage for streaming text, runtime events, approvals, cancellation, failure, and daemon client errors.

## Manual Desktop Scenarios

### 1. Launch And Daemon Status

Run:

```bash
cd crates/iota-desktop
npm run tauri dev
```

Expected:

- The app opens to the Chat-first workbench.
- The daemon status changes to connected after config loads.
- If the daemon is not already running, the Tauri backend autostarts it through the configured CLI path.
- No secret values are shown in the header or logs.

### 2. Backend Readiness

Steps:

- Open the backend selector.
- Select one configured backend and one intentionally unavailable or unconfigured backend if available.

Expected:

- Configured backend is marked ready and allows prompt submission.
- Unavailable backend is marked with a clear reason such as missing API key, missing ACP command, disabled backend, or missing config section.
- Send button stays disabled for the unavailable backend.

### 3. Config Panel

Steps:

- Open the Config view.
- Confirm model/provider/base URL values are visible.
- Confirm API keys are masked as configured/missing, not displayed literally.
- Save a harmless model field change or re-save an existing value.

Expected:

- Config is loaded from daemon APIs.
- Save goes through daemon APIs and writes `~/.i6/nimia.yaml` semantics.
- Backend readiness refreshes after save.
- Hermes behavior remains unchanged; the desktop app does not set or override `HERMES_HOME`.

### 4. Successful Prompt Streaming

Steps:

- Select a configured backend.
- Submit a small prompt such as `Say hello in one short sentence.`

Expected:

- A turn appears in the transcript immediately.
- Assistant text streams into the transcript or appears at completion if the backend only sends final text.
- The right inspector shows running status, then completed status.
- Prompt submission is disabled while the active turn is running and enabled after completion.

### 5. Inspector Details

Steps:

- Select the completed turn.
- Review the right inspector.

Expected:

- Timing summary is shown when available.
- Token usage is shown when the backend reports usage.
- Runtime events are retained for the turn.
- Tool calls and tool results appear when the backend emits them.
- Long JSON payloads remain scrollable and do not break layout.

### 6. Approval Approve And Deny

Steps:

- Use a prompt/backend/tool combination known to request permission.
- When approval appears, approve it once.
- Run a second permission-triggering prompt and deny it.

Expected:

- Approval is shown in the right inspector with tool name and params.
- Approve sends the decision through daemon approval APIs.
- Deny sends the decision through daemon approval APIs.
- Lost or closed approval streams fail closed and do not auto-approve.
- Turn state leaves waiting approval after a terminal result/failure/cancellation.

### 7. Cancellation

Steps:

- Start a longer running prompt.
- Click Interrupt Execution.

Expected:

- The daemon receives `CancelTurn`.
- The turn is marked cancelled in the transcript and inspector.
- Partial text and events remain visible.
- Prompt submission unlocks after cancellation.

### 8. Stream Interruption Or Daemon Disconnect

Steps:

- Start a running prompt.
- Stop the daemon process or otherwise interrupt the stream.

Expected:

- The frontend receives a daemon client error.
- The active turn is marked failed while preserving partial text/events.
- Daemon status shows error.
- The app remains usable after restarting/reconnecting where possible.

### 9. CLI Daemon Compatibility

Run after desktop use:

```bash
cargo run -p iota-cli -- check --daemon
cargo run -p iota-cli -- run --daemon gemini "ping"
```

Expected:

- CLI daemon request/response still works.
- The desktop streaming protocol did not break the legacy CLI path.
- Output does not expose secrets.

## Acceptance Result

Record the date, OS, backend used, and result here when executing the runbook.

| Date | OS | Backend | Result | Notes |
| :--- | :--- | :--- | :--- | :--- |
| pending | pending | pending | pending | Not yet manually executed |

## Non-Blocking Follow-Ups

Use this section only for issues that do not invalidate the Chat-first daemon MVP baseline.

| Item | Scope | Owner |
| :--- | :--- | :--- |
| pending | pending | pending |
