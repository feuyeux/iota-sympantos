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
  | { type: "observability_summary"; summary: any }
  | { type: "memory_context_snapshot"; snapshot: DesktopMemoryContextSnapshot };

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

export type KanbanStatus = "triage" | "todo" | "ready" | "running" | "blocked" | "done" | "archived";

export type KanbanBoard = {
  id: number;
  slug: string;
  name: string;
  created_at: number;
};

export type KanbanTask = {
  id: number;
  board_id: number;
  title: string;
  body?: string;
  status: KanbanStatus;
  assignee?: string;
  priority: number;
  tags: string[];
  workspace_kind?: string;
  workspace_path?: string;
  created_at: number;
  updated_at: number;
  claimed_at?: number;
  claim_ttl_secs: number;
};

export type KanbanTaskPatch = {
  title?: string;
  body?: string | null;
  status?: KanbanStatus;
  assignee?: string | null;
  priority?: number;
  tags?: string[];
  workspace_kind?: string | null;
  workspace_path?: string | null;
};

export type KanbanComment = {
  id: number;
  task_id: number;
  author: string;
  body: string;
  created_at: number;
};

export type KanbanRunStatus = "running" | "completed" | "failed" | "timed_out" | "cancelled";

export type KanbanRun = {
  id: string;
  task_id: number;
  profile: string;
  status: KanbanRunStatus;
  started_at: number;
  finished_at?: number;
  last_heartbeat: number;
  exit_code?: number;
  output_summary?: string;
};

export type KanbanLinkKind = "parent" | "blocks" | "related";

export type KanbanLink = {
  from_id: number;
  to_id: number;
  kind: KanbanLinkKind;
};

export type KanbanCreateLinkRequest = {
  from_id: number;
  to_id: number;
  kind: KanbanLinkKind;
};

export type KanbanEvent = {
  id: number;
  event_type: string;
  payload: string;
  created_at: number;
};

export type KanbanTaskLogs = {
  stdout_path: string;
  stderr_path: string;
  stdout: string;
  stderr: string;
};

export type KanbanTaskDetail = {
  task: KanbanTask;
  board?: KanbanBoard;
  comments: KanbanComment[];
  runs: KanbanRun[];
  links: KanbanLink[];
  events: KanbanEvent[];
  logs: KanbanTaskLogs;
};

export type KanbanTaskFilter = {
  board_id?: number;
  status?: KanbanStatus;
  assignee?: string;
  limit?: number;
};

export type KanbanDispatchReport = {
  spawned: number;
  completed: number;
  timed_out: number;
  spawn_failures: number;
  reclaimed: number;
  active_workers: number;
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

export type DesktopMemoryRecord = {
  id: string;
  type: string;
  facet?: string;
  scope: string;
  scope_id: string;
  content: string;
  confidence: number;
  created_at: number;
  updated_at: number;
  expires_at: number;
};

export type DesktopMemoryBuckets = {
  identity: DesktopMemoryRecord[];
  preference: DesktopMemoryRecord[];
  strategic: DesktopMemoryRecord[];
  domain: DesktopMemoryRecord[];
  procedural: DesktopMemoryRecord[];
  episodic: DesktopMemoryRecord[];
};

export type DesktopMemorySummary = {
  identity: number;
  preference: number;
  strategic: number;
  domain: number;
  procedural: number;
  episodic: number;
};

export type DesktopContextBudgetsSnapshot = {
  memory_chars: number;
  skills_chars: number;
  working_memory_chars: number;
  workspace_chars: number;
  handoff_chars: number;
};

export type DesktopContextSection = {
  name: string;
  chars: number;
  preview: string;
};

export type DesktopRuntimeContextSnapshot = {
  turn_id: string;
  backend: string;
  cwd: string;
  session_id: string;
  model?: string;
  created_at: number;
  capsule_text: string;
  sections: DesktopContextSection[];
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopContextEngineSnapshot = {
  enabled: boolean;
  memory_db?: string;
  budgets: DesktopContextBudgetsSnapshot;
};

export type DesktopSnapshotError = {
  area: string;
  message: string;
};

export type DesktopMemoryContextSnapshot = {
  cwd: string;
  scope_mode: "workspace" | "all";
  memory: DesktopMemoryBuckets;
  memory_summary: DesktopMemorySummary;
  runtime_context?: DesktopRuntimeContextSnapshot;
  context_engine: DesktopContextEngineSnapshot;
  errors: DesktopSnapshotError[];
};
