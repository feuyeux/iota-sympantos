import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DaemonServerMessage, DesktopConfigSnapshot, DesktopModelConfig } from "./types";

export function submitPrompt(prompt: string, backend: string): Promise<string> {
  return invoke<string>("submit_prompt", { prompt, backendStr: backend });
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

export function listenDaemonMessages(callback: (message: DaemonServerMessage) => void): Promise<() => void> {
  return listen<DaemonServerMessage>("daemon-message", (event) => callback(event.payload));
}
