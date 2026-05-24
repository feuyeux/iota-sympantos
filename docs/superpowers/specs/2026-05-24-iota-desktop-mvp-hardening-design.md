# iota-desktop MVP Hardening Design

## Goal

Move `iota-desktop` from a daemon-first implementation baseline to a manually verifiable MVP baseline that is stable enough for feature planning.

The previous phase established the architecture: React and Tauri talk to the local daemon, and the daemon owns `EnginePool`, ACP clients, approvals, config, streaming text, runtime events, and observability. This phase hardens that path so the desktop app can be exercised end-to-end without relying on direct `IotaEngine` integration or undocumented behavior.

## Non-Goals

- Do not make the desktop app Kanban-first.
- Do not introduce a new configuration source outside `~/.i6/nimia.yaml`.
- Do not add project-local config discovery.
- Do not bypass `acp::permission` for approvals.
- Do not redesign the daemon protocol unless a discovered bug requires a compatible extension.
- Do not add broad UI redesign work beyond stability, clarity, and acceptance readiness.

## Current Baseline

The current implementation already has:

- Desktop JSON-line daemon protocol with hello/version negotiation.
- Desktop turn streaming through daemon TCP connections.
- Scoped desktop approval routing keyed by turn/execution id.
- Tauri daemon client with autostart, handshake, one-shot commands, and streaming bridge.
- React chat workbench with turn reducer and right inspector.
- Config panel backed by daemon config APIs.
- Observability summary surfaced through daemon APIs.
- Automated checks passing for `iota-core`, `iota-desktop`, frontend tests, and frontend build.

## Problem Statement

Automated tests now prove the main protocol pieces, but the desktop MVP still lacks a disciplined acceptance layer:

- Manual verification steps are described across older plan text, but not as a single current runbook.
- Stream interruption and daemon disconnect behavior needs explicit UI-visible handling.
- Approval request handling needs manual acceptance coverage, including deny and lost-window behavior.
- Backend readiness and config save flows need acceptance coverage with masked secrets.
- Inspector rendering should be checked against real streamed events, not only reducer unit tests.
- The code needs final lint/format/workspace gates before starting feature planning.

## Target Product Behavior

The desktop app should satisfy these acceptance behaviors:

1. App startup connects to an existing daemon or autostarts one.
2. Backend selector marks unavailable backends clearly without exposing secrets.
3. Config panel reads masked config from daemon and saves model/API key fields through daemon APIs.
4. Prompt submission creates a turn, streams text into the transcript, and updates the active turn inspector.
5. Runtime events update inspector sections for tool calls, approvals, usage, timing, and raw event details where available.
6. Approval requests can be approved or denied, and lost/closed streams fail closed.
7. Cancellation marks the turn cancelled and unlocks prompt submission.
8. Stream disconnection marks the turn failed or disconnected while preserving partial output and events.
9. Existing CLI daemon request/response behavior remains compatible.

## Technical Direction

### Desktop Event Handling

Tauri should continue emitting daemon messages as `daemon-message`. The frontend reducer remains the source of truth for turn state.

Add or harden UI handling for daemon client errors and stream EOF/disconnect events. These should be represented as turn-scoped failures when a turn id is known, or as a global daemon status error when no turn id is known.

### Acceptance Runbook

Create a single manual runbook covering:

- daemon autostart
- successful prompt streaming
- configured/unconfigured backend display
- config save with masked API keys
- approval approve/deny
- cancellation
- right inspector event rendering
- CLI daemon compatibility

The runbook should document expected output/state without requiring secrets to be printed.

### Test Gates

Before this phase is complete, these commands should pass:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cd crates/iota-desktop && npm test && npm run build
```

Manual desktop verification should be run on the primary development OS before declaring the MVP baseline ready.

## Risks

- Real backend approval prompts vary by ACP adapter, so manual approval testing needs at least one backend/tool combination known to request permission.
- Daemon autostart depends on `IOTA_CLI_PATH`, PATH, or sibling executable discovery; packaged desktop behavior may still need later installer-specific work.
- Token usage is backend-dependent and may not appear for every prompt.
- `cargo clippy --workspace --all-targets -- -D warnings` can surface unrelated existing warnings; those should be fixed or explicitly scoped before MVP signoff.

## Completion Criteria

This phase is complete when:

- Automated gates pass.
- Manual runbook exists and has been executed at least once on the primary development machine.
- Any discovered acceptance blockers are fixed or documented as follow-up issues that do not invalidate the Chat-first daemon MVP.
- The next product planning phase can start from a known desktop baseline instead of revalidating daemon-first fundamentals.
