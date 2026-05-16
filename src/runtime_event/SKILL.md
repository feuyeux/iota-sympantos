# runtime_event — Unified Event Types

Normalized event types for telemetry, observability, TUI rendering, and inter-module routing.

## Responsibilities

- Define a unified `RuntimeEvent` enum covering all observable events
- Map raw ACP protocol events to normalized runtime events
- Carry structured metadata for each event type (output, tool calls, errors, memory, approvals, token usage)

## Key Types

- `RuntimeEvent` — enum: Output, State, Log, ToolCall, ToolResult, Error, Memory, ApprovalRequest, ApprovalDecision, TokenUsage
- `OutputEvent` — text output from backends
- `ToolCallEvent` — tool invocation with name/arguments
- `ToolResultEvent` — tool execution result
- `ErrorEvent` — error with context
- `MemoryEvent` — memory write notification
- `StateEvent` — state transitions
- `LogEvent` — structured log entry
- `ApprovalDecisionEvent` — approval request resolution
