import assert from "node:assert/strict";
import test from "node:test";
import { initialTurnsState, turnsReducer } from "./turnReducer";

test("text_chunk appends assistant text for the matching turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "hi" },
  });

  assert.equal(updated.turns["turn-1"].assistantText, "hi");
  assert.equal(updated.turns["turn-1"].status, "running");
});

test("approval_requested marks the turn as waiting for approval", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "gemini",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "approval_requested",
      turn_id: "turn-1",
      approval_id: "approval-1",
      tool_name: "shell",
      params: { command: "ls" },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "waiting_approval");
  assert.equal(updated.turns["turn-1"].approvals[0].id, "approval-1");
});

test("turn_completed stores timing and completes the turn", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });

  const updated = turnsReducer(started, {
    type: "daemon_message",
    message: {
      type: "turn_completed",
      turn_id: "turn-1",
      text: "final",
      timing: { total_ms: 12 },
    },
  });

  assert.equal(updated.turns["turn-1"].status, "completed");
  assert.equal(updated.turns["turn-1"].assistantText, "final");
});

test("turn_failed preserves partial text and stores error", () => {
  const started = turnsReducer(initialTurnsState, {
    type: "turn_started",
    turnId: "turn-1",
    backend: "codex",
    cwd: "/tmp/project",
    prompt: "hello",
  });
  const chunked = turnsReducer(started, {
    type: "daemon_message",
    message: { type: "text_chunk", turn_id: "turn-1", chunk: "partial" },
  });

  const failed = turnsReducer(chunked, {
    type: "daemon_message",
    message: { type: "turn_failed", turn_id: "turn-1", error: "boom" },
  });

  assert.equal(failed.turns["turn-1"].status, "failed");
  assert.equal(failed.turns["turn-1"].assistantText, "partial");
  assert.equal(failed.turns["turn-1"].error, "boom");
});
