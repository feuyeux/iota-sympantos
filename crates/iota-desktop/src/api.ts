import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  BackendCheckResult,
  DaemonClientError,
  DaemonServerMessage,
  DesktopConfigSnapshot,
  KanbanBoard,
  KanbanDispatchReport,
  KanbanTask,
  KanbanTaskDetail,
  KanbanTaskFilter,
  DesktopModelConfig,
  ObservabilitySummary,
  DesktopMemoryContextSnapshot,
} from "./types";

export function submitPrompt(prompt: string, backend: string, turnId: string): Promise<string> {
  return invoke<string>("submit_prompt", { prompt, backendStr: backend, turnId });
}

export function getConfig(): Promise<DesktopConfigSnapshot> {
  return invoke<DesktopConfigSnapshot>("get_config");
}

export function saveBackendModel(backend: string, model: DesktopModelConfig): Promise<DesktopConfigSnapshot> {
  return invoke<DesktopConfigSnapshot>("save_backend_model", { backendStr: backend, model });
}

export function handleApproval(reqId: string, approved: boolean): Promise<void> {
  return invoke<void>("handle_approval", { reqId, approved });
}

export function cancelTurn(turnId: string): Promise<void> {
  return invoke<void>("cancel_turn", { turnId });
}

export async function checkBackend(backend: string): Promise<BackendCheckResult> {
  const message = await invoke<DaemonServerMessage>("check_backend", { backendStr: backend });
  if (message.type !== "backend_check_result") {
    throw new Error("daemon returned an unexpected backend check response");
  }
  return { backend: message.backend, ok: message.ok, details: message.details };
}

export function getObservabilitySummary(): Promise<ObservabilitySummary> {
  return invoke<ObservabilitySummary>("get_observability_summary");
}

export function currentWorkspace(): Promise<string> {
  return invoke<string>("current_workspace");
}

export function listenDaemonMessages(callback: (message: DaemonServerMessage) => void): Promise<() => void> {
  return listen<DaemonServerMessage>("daemon-message", (event) => callback(event.payload));
}

export function listenDaemonClientErrors(callback: (error: DaemonClientError) => void): Promise<() => void> {
  return listen<DaemonClientError>("daemon-client-error", (event) => callback(event.payload));
}

export function getMemoryContextSnapshot(scopeMode: "workspace" | "all"): Promise<DesktopMemoryContextSnapshot> {
  return invoke<DesktopMemoryContextSnapshot>("get_memory_context_snapshot", { scopeMode });
}

export function listKanbanBoards(): Promise<KanbanBoard[]> {
  return invoke<KanbanBoard[]>("list_boards");
}

export function listKanbanTasks(filter: KanbanTaskFilter = {}): Promise<KanbanTask[]> {
  return invoke<KanbanTask[]>("list_tasks", { filter });
}

export function dispatchKanban(): Promise<KanbanDispatchReport> {
  return invoke<KanbanDispatchReport>("dispatch_kanban");
}

export function getKanbanTaskDetail(taskId: number): Promise<KanbanTaskDetail> {
  return invoke<KanbanTaskDetail>("get_kanban_task_detail", { taskId });
}

export function listenKanbanUpdates(callback: (report: KanbanDispatchReport) => void): Promise<() => void> {
  return listen<KanbanDispatchReport>("kanban-updated", (event) => callback(event.payload));
}
