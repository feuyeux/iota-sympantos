import { useCallback, useEffect, useMemo, useReducer, useState } from "react";
import { Cpu, Send, CheckCircle2 } from "lucide-react";
import {
  checkBackend,
  currentWorkspace,
  getConfig,
  getObservabilitySummary,
  listenDaemonClientErrors,
  listenDaemonMessages,
  submitPrompt,
} from "../api";
import { initialTurnsState, turnsReducer } from "../turnReducer";
import { RightInspector } from "./RightInspector";
import { ConfigPanel } from "./ConfigPanel";
import { MemoryContextWorkspace } from "./MemoryContextWorkspace";
import type { BackendCheckResult, DesktopConfigSnapshot, ObservabilitySummary } from "../types";

const BACKENDS = ["gemini", "claude", "hermes", "codex", "opencode"];

export function ChatWorkbench() {
  const [state, dispatch] = useReducer(turnsReducer, initialTurnsState);
  const [backend, setBackend] = useState("gemini");
  const [input, setInput] = useState("");
  const [view, setView] = useState<"chat" | "config" | "memory">("chat");
  const [config, setConfig] = useState<DesktopConfigSnapshot | null>(null);
  const [backendChecks, setBackendChecks] = useState<Record<string, BackendCheckResult>>({});
  const [observability, setObservability] = useState<ObservabilitySummary | null>(null);
  const [workspace, setWorkspace] = useState("");
  const [daemonStatus, setDaemonStatus] = useState<"connecting" | "connected" | "error">("connecting");

  const activeTurn = state.activeTurnId ? state.turns[state.activeTurnId] : undefined;

  const refreshBackendChecks = useCallback(async () => {
    const checks = await Promise.all(
      BACKENDS.map(async (item) => {
        try {
          return [item, await checkBackend(item)] as const;
        } catch (err) {
          return [item, { backend: item, ok: false, details: String(err) }] as const;
        }
      }),
    );
    setBackendChecks(Object.fromEntries(checks));
  }, []);

  const refreshObservability = useCallback(async () => {
    try {
      setObservability(await getObservabilitySummary());
    } catch (err) {
      console.error("Failed to load observability summary:", err);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let unlistenErrors: (() => void) | undefined;

    getConfig()
      .then((cfg) => {
        if (!disposed) {
          setConfig(cfg);
          setDaemonStatus("connected");
        }
      })
      .catch((err) => {
        if (!disposed) setDaemonStatus("error");
        console.error("Failed to load daemon config:", err);
      });

    currentWorkspace()
      .then((cwd) => {
        if (!disposed) setWorkspace(cwd);
      })
      .catch((err) => console.error("Failed to read workspace:", err));

    refreshObservability();
    refreshBackendChecks();

    // Listen for stream events
    listenDaemonMessages((message) => {
      if (disposed) return;
      dispatch({ type: "daemon_message", message });

      // If the message contains a config update, refresh local state
      if (message.type === "config_snapshot") {
        setConfig(message.config);
        refreshBackendChecks();
      }
      if (
        message.type === "turn_completed" ||
        message.type === "turn_failed" ||
        message.type === "turn_cancelled"
      ) {
        refreshObservability();
      }
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup();
        } else {
          unlisten = cleanup;
        }
      })
      .catch((err) => console.error("Failed to listen for daemon messages:", err));

    listenDaemonClientErrors((error) => {
      if (disposed) return;
      dispatch({ type: "daemon_client_error", error });
      setDaemonStatus("error");
    })
      .then((cleanup) => {
        if (disposed) {
          cleanup();
        } else {
          unlistenErrors = cleanup;
        }
      })
      .catch((err) => console.error("Failed to listen for daemon client errors:", err));

    return () => {
      disposed = true;
      unlisten?.();
      unlistenErrors?.();
    };
  }, [refreshBackendChecks, refreshObservability]);

  const transcript = useMemo(() => state.order.map((id) => state.turns[id]), [state.order, state.turns]);
  const activeTurnBusy =
    activeTurn?.status === "queued" ||
    activeTurn?.status === "running" ||
    activeTurn?.status === "waiting_approval";

  async function onSubmit(event: React.FormEvent) {
    event.preventDefault();
    const prompt = input.trim();
    if (!prompt || activeTurnBusy || !canSubmit) return;
    setInput("");
    const turnId = crypto.randomUUID();
    dispatch({
      type: "turn_started",
      turnId,
      backend,
      cwd: workspace,
      prompt,
    });
    try {
      await submitPrompt(prompt, backend, turnId);
    } catch (err) {
      dispatch({
        type: "daemon_message",
        message: {
          type: "turn_failed",
          turn_id: turnId,
          error: err instanceof Error ? err.message : String(err),
        },
      });
    }
  }

  // Extract model details
  const activeBackendSnapshot = config?.backends[backend];
  const modelName = activeBackendSnapshot?.model?.name || "Loading model...";
  const isKeyConfigured = activeBackendSnapshot?.model?.api_key_configured;
  const activeBackendCheck = backendChecks[backend];
  const selectedBackendReady = activeBackendCheck?.ok === true;
  const daemonError = state.pendingError;
  const canSubmit = !daemonError && selectedBackendReady;
  const daemonStatusText =
    daemonStatus === "connected"
      ? "Daemon Connected"
      : daemonStatus === "connecting"
        ? "Connecting"
        : "Daemon Error";

  const handleConfigUpdate = (updated: DesktopConfigSnapshot) => {
    setConfig(updated);
    refreshBackendChecks();
  };

  return (
    <div className="flex h-screen bg-[#0b0f19] text-gray-100 font-sans select-none">
      <main className="flex min-w-0 flex-1 flex-col">
        {/* Header Bar */}
        <header className="flex items-center justify-between border-b border-white/10 bg-[#070a13] px-5 py-3 shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-md bg-primary shadow-lg shadow-primary/20">
              <Cpu className="h-5 w-5 text-white" />
            </div>
            <div>
              <h1 className="text-sm font-semibold flex items-center gap-2">
                Iota Desktop
                <span className="flex items-center gap-1 text-[10px] text-emerald-400 bg-emerald-500/10 px-1.5 py-0.5 rounded-full font-medium">
                  <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-ping" />
                  ● {daemonStatusText}
                </span>
              </h1>
              <p className="text-[11px] text-gray-500 font-mono truncate max-w-xs md:max-w-md">
                Model: {modelName} {isKeyConfigured ? "· API key ✓" : "· API key ✗"}
              </p>
              <p className="text-[10px] text-gray-600 font-mono truncate max-w-xs md:max-w-md" title={workspace}>
                {workspace || "Workspace unavailable"}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <nav className="flex items-center bg-white/[0.03] border border-white/5 p-1 rounded-md">
              <button
                className={`rounded px-3 py-1 text-xs font-semibold transition-all ${
                  view === "chat" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
                }`}
                onClick={() => setView("chat")}
              >
                Chat
              </button>
              <button
                className={`rounded px-3 py-1 text-xs font-semibold transition-all ${
                  view === "memory" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
                }`}
                onClick={() => setView("memory")}
              >
                Memory &amp; Context
              </button>
              <button
                className={`rounded px-3 py-1 text-xs font-semibold transition-all ${
                  view === "config" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
                }`}
                onClick={() => setView("config")}
              >
                Config
              </button>
            </nav>

            <select
              value={backend}
              onChange={(event) => setBackend(event.target.value)}
              className="rounded-md border border-white/10 bg-white/[0.04] hover:bg-white/[0.08] px-3 py-1.5 text-xs text-gray-200 ml-3 focus:outline-none focus:border-primary font-medium cursor-pointer"
            >
              {BACKENDS.map((item) => (
                <option key={item} value={item} className="bg-[#0b0f19]">
                  {item.toUpperCase()} {backendChecks[item]?.ok === false ? "⚠" : ""}
                </option>
              ))}
            </select>
          </div>
        </header>

        {/* Content Panel */}
        {view === "chat" ? (
          <>
            {/* Messages Scroll Area */}
            <div className="flex-1 overflow-y-auto p-5 space-y-4 select-text">
              {transcript.length === 0 ? (
                <div className="flex h-full flex-col items-center justify-center text-sm text-gray-500 gap-2">
                  <CheckCircle2 className="h-8 w-8 text-gray-700" />
                  <span>
                    {daemonError
                      ? daemonError
                      : selectedBackendReady
                        ? `Send a prompt to begin coding with ${backend.toUpperCase()}`
                        : `${backend.toUpperCase()} is not ready: ${activeBackendCheck?.details ?? "checking configuration"}`}
                  </span>
                </div>
              ) : null}
              {transcript.map((turn) => {
                const isSelected = state.activeTurnId === turn.id;
                return (
                  <div
                    key={turn.id}
                    onClick={() => dispatch({ type: "select_active_turn", turnId: turn.id })}
                    className={`p-3 rounded-lg cursor-pointer transition-all border ${
                      isSelected
                        ? "border-primary/40 bg-white/[0.02] shadow-sm shadow-primary/5"
                        : "border-transparent hover:bg-white/[0.01]"
                    }`}
                  >
                    <div className="mb-2.5 flex justify-end">
                      <div className="max-w-[72ch] rounded-lg bg-primary px-4 py-2.5 text-sm text-white shadow shadow-primary/10">
                        {turn.userPrompt}
                      </div>
                    </div>
                    <div className="flex flex-col items-start gap-1">
                      <span className="text-[10px] font-mono text-gray-500 px-1">{turn.backend.toUpperCase()}</span>
                      <div className="max-w-[88ch] rounded-lg border border-white/5 bg-white/[0.03] px-4 py-3 text-sm leading-6 text-gray-200 whitespace-pre-wrap font-sans">
                        {turn.assistantText ||
                          (turn.status === "failed" ? (
                            <span className="text-rose-400 font-medium">{turn.error}</span>
                          ) : turn.status === "queued" ? (
                            <span className="text-gray-500 italic">Queued...</span>
                          ) : (
                            <span className="text-blue-400 flex items-center gap-1.5 font-medium">
                              <span className="h-1.5 w-1.5 rounded-full bg-blue-400 animate-ping" />
                              Running...
                            </span>
                          ))}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>

            {/* Prompt Form */}
            <form onSubmit={onSubmit} className="border-t border-white/10 bg-[#070a13] p-4 shrink-0">
              <div className="flex gap-3">
                <textarea
                  value={input}
                  onChange={(event) => setInput(event.target.value)}
                  rows={2}
                  className="min-h-[60px] flex-1 resize-none rounded-md border border-white/10 bg-white/[0.04] px-4 py-2.5 text-sm text-gray-100 outline-none focus:border-primary transition-all font-sans"
                  placeholder={
                    activeTurnBusy
                      ? "Wait for the active turn to finish or interrupt it..."
                      : daemonError
                        ? daemonError
                      : !selectedBackendReady
                        ? activeBackendCheck?.details ?? "Checking selected backend..."
                      : `Ask ${backend.toUpperCase()} to write code, debug, or solve tasks...`
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      onSubmit(e);
                    }
                  }}
                />
                <button
                  type="submit"
                  disabled={!input.trim() || activeTurnBusy || !canSubmit}
                  className="flex h-[60px] w-12 items-center justify-center rounded-md bg-primary hover:bg-primary/95 text-white disabled:opacity-50 transition-colors shadow shadow-primary/25 cursor-pointer"
                >
                  <Send className="h-4.5 w-4.5" />
                </button>
              </div>
            </form>
          </>
        ) : view === "config" ? (
          <ConfigPanel config={config} backendChecks={backendChecks} onConfigUpdate={handleConfigUpdate} />
        ) : (
          <MemoryContextWorkspace />
        )}
      </main>

      {/* Side Inspector Panel */}
      <RightInspector
        turn={activeTurn}
        observability={observability}
        onApprovalDecision={(approvalId, approved) =>
          dispatch({ type: "approval_decision", approvalId, approved })
        }
        onOpenMemoryViewer={() => setView("memory")}
      />
    </div>
  );
}
