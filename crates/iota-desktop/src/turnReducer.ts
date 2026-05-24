import type { DaemonServerMessage, DesktopTurn, RuntimeEventView, ToolCallView } from "./types";

export type TurnsState = {
  activeTurnId?: string;
  turns: Record<string, DesktopTurn>;
  order: string[];
  pendingError?: string;
};

export const initialTurnsState: TurnsState = {
  turns: {},
  order: [],
};

export type TurnsAction =
  | { type: "turn_started"; turnId: string; backend: string; cwd: string; prompt: string }
  | { type: "daemon_message"; message: DaemonServerMessage }
  | { type: "approval_decision"; approvalId: string; approved: boolean }
  | { type: "select_active_turn"; turnId: string };

export function turnsReducer(state: TurnsState, action: TurnsAction): TurnsState {
  if (action.type === "select_active_turn") {
    return {
      ...state,
      activeTurnId: action.turnId,
    };
  }

  if (action.type === "turn_started") {
    const turn: DesktopTurn = {
      id: action.turnId,
      backend: action.backend,
      cwd: action.cwd,
      status: "queued",
      userPrompt: action.prompt,
      assistantText: "",
      events: [],
      toolCalls: [],
      approvals: [],
    };
    return {
      ...state,
      activeTurnId: action.turnId,
      order: [...state.order, action.turnId],
      turns: { ...state.turns, [action.turnId]: turn },
    };
  }

  if (action.type === "approval_decision") {
    return mapTurns(state, (turn) => ({
      ...turn,
      approvals: turn.approvals.map((approval) =>
        approval.id === action.approvalId
          ? { ...approval, status: action.approved ? "approved" : "denied" }
          : approval,
      ),
    }));
  }

  const message = action.message;
  if (message.type === "protocol_error") {
    return { ...state, pendingError: message.message };
  }
  if (!("turn_id" in message)) {
    return state;
  }

  const existing = state.turns[message.turn_id];
  if (!existing) return state;

  const updated = reduceTurn(existing, message);
  return {
    ...state,
    activeTurnId: message.turn_id,
    turns: { ...state.turns, [message.turn_id]: updated },
  };
}

function reduceTurn(turn: DesktopTurn, message: Extract<DaemonServerMessage, { turn_id: string }>): DesktopTurn {
  switch (message.type) {
    case "turn_started":
      return { ...turn, status: "running" };
    case "text_chunk":
      return { ...turn, status: "running", assistantText: turn.assistantText + message.chunk };
    case "turn_event":
      return applyRuntimeEvent({ ...turn, events: [...turn.events, message.event] }, message.event);
    case "approval_requested":
      return {
        ...turn,
        status: "waiting_approval",
        approvals: [
          ...turn.approvals,
          { id: message.approval_id, toolName: message.tool_name, params: message.params, status: "pending" },
        ],
      };
    case "turn_completed":
      return { ...turn, status: "completed", assistantText: message.text, timing: message.timing };
    case "turn_failed":
      return { ...turn, status: "failed", error: message.error };
    case "turn_cancelled":
      return { ...turn, status: "cancelled" };
  }
}

function applyRuntimeEvent(turn: DesktopTurn, event: RuntimeEventView): DesktopTurn {
  if (event.kind === "TokenUsage") {
    return { ...turn, usage: event.data };
  }
  if (event.kind === "ToolCall" && isObject(event.data)) {
    const toolCall: ToolCallView = {
      id: String(event.data.id ?? ""),
      name: String(event.data.name ?? ""),
      arguments: event.data.arguments,
    };
    return { ...turn, toolCalls: [...turn.toolCalls, toolCall] };
  }
  if (event.kind === "ToolResult" && isObject(event.data)) {
    return {
      ...turn,
      toolCalls: turn.toolCalls.map((tool) =>
        tool.id === event.data.id
          ? { ...tool, ok: Boolean(event.data.ok), result: event.data.result }
          : tool,
      ),
    };
  }
  return turn;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function mapTurns(state: TurnsState, f: (turn: DesktopTurn) => DesktopTurn): TurnsState {
  const turns: Record<string, DesktopTurn> = {};
  for (const id of Object.keys(state.turns)) {
    turns[id] = f(state.turns[id]);
  }
  return { ...state, turns };
}
