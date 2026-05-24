# iota-desktop MVP Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for task-by-task execution. Keep checkboxes updated as work lands.

**Goal:** Turn the daemon-first desktop implementation into a manually verifiable MVP baseline that is ready for the next product planning phase.

**Architecture:** Preserve the daemon-first flow: React reducer -> Tauri daemon client -> local TCP daemon -> `EnginePool` / `IotaEngine` / ACP. This plan hardens event handling, acceptance coverage, and verification gates without changing the product shape.

**Reference Design:** `docs/superpowers/specs/2026-05-24-iota-desktop-mvp-hardening-design.md`

---

## Task 1: Audit Current Desktop Baseline

**Files:**
- Inspect: `crates/iota-core/src/daemon/desktop.rs`
- Inspect: `crates/iota-desktop/src-tauri/src/daemon_client.rs`
- Inspect: `crates/iota-desktop/src-tauri/src/lib.rs`
- Inspect: `crates/iota-desktop/src/turnReducer.ts`
- Inspect: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- Inspect: `crates/iota-desktop/src/components/RightInspector.tsx`
- Inspect: `crates/iota-desktop/src/components/ConfigPanel.tsx`

- [x] **Step 1: Confirm daemon-first dependency boundary**

Run:

```bash
rg -n "IotaEngine|create_session|run_with_timing|run\(" crates/iota-desktop/src-tauri/src -S
```

Expected: desktop Tauri code does not construct or call `IotaEngine` directly. Any hits should be imports from `iota_core::daemon` or unrelated test names.

- [x] **Step 2: Confirm daemon protocol coverage**

Run:

```bash
cargo test -p iota-core daemon -- --nocapture
```

Expected: desktop protocol serde, hello rejection, approval registry, backend checks, and legacy daemon tests pass.

- [x] **Step 3: Record remaining acceptance gaps**

Create or update a short checklist in `docs/desktop-mvp-acceptance.md` with the manual scenarios from Task 4 below. Do not include secrets or API keys.

---

## Task 2: Harden Desktop Error And Disconnect Handling

**Files:**
- Modify: `crates/iota-desktop/src/types.ts`
- Modify: `crates/iota-desktop/src/turnReducer.ts`
- Modify: `crates/iota-desktop/src/turnReducer.test.ts`
- Modify: `crates/iota-desktop/src-tauri/src/daemon_client.rs`
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`

- [x] **Step 1: Add explicit daemon client event type if needed**

Review how `daemon-client-error` is emitted today. If the UI only logs it, add a typed frontend event path that converts daemon client errors into either:

- a turn-scoped failure when a `turn_id` is known
- a global daemon status error otherwise

Keep the reducer pure and avoid parsing rendered text.

- [x] **Step 2: Add reducer tests for disconnect/error**

Extend `turnReducer.test.ts` to cover:

- stream disconnect while running preserves partial text and marks failed/disconnected
- daemon protocol error is surfaced in `pendingError`
- cancellation unlocks active turn flow

Run:

```bash
cd crates/iota-desktop && npm test
```

- [x] **Step 3: Ensure daemon client emits useful errors**

When the stream reader exits unexpectedly before a terminal turn message, emit an event that includes the `turn_id` supplied to `start_turn`.

Run:

```bash
cargo test -p iota-desktop
cd crates/iota-desktop && npm test
```

---

## Task 3: Harden Inspector Rendering

**Files:**
- Modify: `crates/iota-desktop/src/components/RightInspector.tsx`
- Modify: `crates/iota-desktop/src/components/ChatWorkbench.tsx`
- Modify: `crates/iota-desktop/src/turnReducer.ts`
- Modify: `crates/iota-desktop/src/turnReducer.test.ts`

- [x] **Step 1: Verify runtime event mapping**

Confirm reducer support for these event kinds:

- `ToolCall`
- `ToolResult`
- `TokenUsage`
- `ApprovalRequest` / `ApprovalDecision` if surfaced through runtime events
- generic events retained in `events`

- [x] **Step 2: Improve raw event display if needed**

Right inspector should show high-value summaries by default and keep raw JSON folded/scrollable. It should not let long raw payloads break layout.

- [x] **Step 3: Add focused reducer tests**

Add tests proving `ToolCall`, `ToolResult`, and `TokenUsage` update inspector-friendly state.

Run:

```bash
cd crates/iota-desktop && npm test
```

---

## Task 4: Create Desktop MVP Acceptance Runbook

**Files:**
- Create: `docs/desktop-mvp-acceptance.md`
- Modify if needed: `crates/iota-desktop/README.md`

- [x] **Step 1: Document prerequisites**

Include:

- configured `~/.i6/nimia.yaml`
- at least one backend with API key configured
- `IOTA_CLI_PATH` or `iota` in PATH for daemon autostart
- warning that secrets must not be pasted into logs

- [x] **Step 2: Document automated gates**

Include:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p iota-cli -- check
cd crates/iota-desktop && npm test && npm run build
```

- [x] **Step 3: Document manual desktop scenarios**

Include expected behavior for:

- app opens to Chat-first workbench
- daemon connects or autostarts
- backend readiness display
- config panel masked read and save
- successful prompt streaming
- right inspector status/events/timing/usage/tool calls
- approval approve and deny
- cancellation
- stream interruption / daemon disconnect
- CLI daemon compatibility after desktop use

- [x] **Step 4: Link runbook from README**

Add a short README link so future workers know where manual acceptance lives.

---

## Task 5: Run Final Automated Gates

**Files:**
- No code changes expected unless gates fail.

- [x] **Step 1: Rust format check**

```bash
cargo fmt --all --check
```

- [x] **Step 2: Workspace tests**

```bash
cargo test --workspace
```

- [x] **Step 3: Clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [x] **Step 4: CLI check**

```bash
cargo run -p iota-cli -- check
```

- [x] **Step 5: Frontend tests and build**

```bash
cd crates/iota-desktop
npm test
npm run build
```

Expected: all automated gates pass. If `cargo run -p iota-cli -- check` depends on local credentials, it must still avoid printing secrets.

---

## Task 6: Run Manual Desktop Acceptance

**Files:**
- Update: `docs/desktop-mvp-acceptance.md` with date/result notes if the project wants tracked acceptance evidence.

- [ ] **Step 1: Launch desktop**

```bash
cd crates/iota-desktop
npm run tauri dev
```

- [ ] **Step 2: Execute runbook scenarios**

Follow `docs/desktop-mvp-acceptance.md`.

- [ ] **Step 3: Fix blockers or log non-blocking follow-ups**

Acceptance blockers must be fixed before marking this phase complete. Non-blocking follow-ups should be concrete and scoped.

---

## Completion Checklist

- [x] Desktop still uses daemon path only.
- [x] Stream completion/failure/cancellation leaves UI in a terminal state.
- [ ] Approval approve and deny are manually verified.
- [x] Config reads and saves through daemon APIs only.
- [x] Runtime events/timing/usage/tool calls render in the inspector when produced.
- [x] Automated gates pass.
- [ ] Manual acceptance runbook exists and has been executed.
- [ ] Remaining work is small enough to plan as product features rather than daemon-first architecture repair.
