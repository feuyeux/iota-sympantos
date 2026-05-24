export type TurnStatus = "queued" | "running" | "waiting_approval" | "completed" | "failed" | "cancelled";

export type RuntimeEventView = {
  kind: string;
  data: any;
};

export type ApprovalView = {
  id: string;
  toolName: string;
  params: any;
  status: "pending" | "approved" | "denied";
};

export type ToolCallView = {
  id: string;
  name: string;
  arguments: any;
  ok?: boolean;
  result?: any;
};

export type DesktopTurn = {
  id: string;
  backend: string;
  cwd: string;
  status: TurnStatus;
  userPrompt: string;
  assistantText: string;
  events: RuntimeEventView[];
  toolCalls: ToolCallView[];
  approvals: ApprovalView[];
  timing?: any;
  usage?: any;
  error?: string;
};

export type DaemonServerMessage =
  | { type: "hello_accepted"; protocol_version: number }
  | { type: "protocol_error"; message: string }
  | { type: "turn_started"; turn_id: string }
  | { type: "text_chunk"; turn_id: string; chunk: string }
  | { type: "turn_event"; turn_id: string; event: RuntimeEventView }
  | { type: "approval_requested"; turn_id: string; approval_id: string; tool_name: string; params: any }
  | { type: "approval_responded"; approval_id: string; accepted: boolean }
  | { type: "turn_completed"; turn_id: string; text: string; timing: any }
  | { type: "turn_failed"; turn_id: string; error: string }
  | { type: "turn_cancelled"; turn_id: string; accepted: boolean }
  | { type: "config_snapshot"; config: DesktopConfigSnapshot }
  | { type: "backend_check_result"; backend: string; ok: boolean; details: string }
  | { type: "observability_summary"; summary: any };

export type DaemonClientError = {
  turn_id?: string;
  message: string;
};

export type DesktopModelConfig = {
  provider?: string;
  name?: string;
  base_url?: string;
  api_key_configured: boolean;
  api_key_update?: string;
};

export type DesktopBackendSnapshot = {
  backend: string;
  enabled: boolean;
  model?: DesktopModelConfig;
};

export type DesktopConfigSnapshot = {
  config_path: string;
  backends: Record<string, DesktopBackendSnapshot>;
};

export type BackendCheckResult = {
  backend: string;
  ok: boolean;
  details: string;
};

export type ObservabilitySummary = {
  cwd?: string;
  window_secs?: number;
  token_summary?: Array<{
    backend: string;
    count: number;
    normalized_total_mean?: number;
    input_tokens_mean?: number;
    output_tokens_mean?: number;
  }>;
  recent_token_executions?: Array<{
    id: string;
    ts: number;
    execution_id?: string;
    backend: string;
    model?: string;
    normalized_total_tokens?: number;
  }>;
  error?: string;
};
