import {
  AlertCircle,
  Clock,
  Terminal,
  Zap,
  Ban,
  Coins,
  Activity,
  ShieldAlert,
  Cpu,
  CheckCircle2,
  Database,
} from "lucide-react";
import type { DesktopTurn, ObservabilitySummary } from "../types";
import { handleApproval, cancelTurn } from "../api";
import { useState } from "react";
import { MemoryContextWorkspace } from "./MemoryContextWorkspace";

type Props = {
  turn?: DesktopTurn;
  observability?: ObservabilitySummary | null;
  onApprovalDecision: (approvalId: string, approved: boolean) => void;
  width?: number;
};

type InspectorTab = "observability" | "memory" | "context";

export function RightInspector({ turn, observability, onApprovalDecision, width = 640 }: Props) {
  const [cancelling, setCancelling] = useState(false);
  const [activeTab, setActiveTab] = useState<InspectorTab>("observability");

  const formatMs = (ms?: number) => {
    if (ms === undefined) return "N/A";
    return `${(ms / 1000).toFixed(2)}s`;
  };

  const formatNumber = (num?: number) => {
    if (num === undefined) return "0";
    return new Intl.NumberFormat().format(num);
  };

  const renderObservabilitySection = () => (
    <section className="bg-white/[0.01] border border-white/5 rounded-md p-4">
      <div className="flex items-center gap-2 text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">
        <Activity className="h-4 w-4 text-primary" />
        Recent Observability
      </div>
      {observability?.token_summary && observability.token_summary.length > 0 ? (
        <div className="space-y-2 text-xs text-gray-300">
          {observability.token_summary.slice(0, 5).map((summary) => (
            <div key={summary.backend} className="rounded border border-white/5 bg-white/[0.02] p-2">
              <div className="flex items-center justify-between">
                <span className="font-medium uppercase">{summary.backend}</span>
                <span className="text-gray-500">{summary.count} turns</span>
              </div>
              <div className="mt-1 grid grid-cols-3 gap-2 text-[11px] text-gray-500">
                <div>
                  <span>Input</span>
                  <div className="text-gray-300">{formatNumber(Math.round(summary.input_tokens_mean ?? 0))}</div>
                </div>
                <div>
                  <span>Output</span>
                  <div className="text-gray-300">{formatNumber(Math.round(summary.output_tokens_mean ?? 0))}</div>
                </div>
                <div>
                  <span>Total</span>
                  <div className="text-gray-300">{formatNumber(Math.round(summary.normalized_total_mean ?? 0))}</div>
                </div>
              </div>
            </div>
          ))}
          {observability.recent_token_executions && observability.recent_token_executions.length > 0 ? (
            <div className="border-t border-white/5 pt-2">
              <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-gray-500">Recent Runs</div>
              <div className="space-y-1.5">
                {observability.recent_token_executions.slice(0, 3).map((execution) => (
                  <div key={execution.id} className="flex items-center justify-between text-[11px] text-gray-500">
                    <span className="truncate uppercase">{execution.backend}</span>
                    <span className="font-mono text-gray-400">
                      {formatNumber(execution.normalized_total_tokens)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="text-xs text-gray-600 italic">
          {observability?.error ?? "No recent token usage summary"}
        </div>
      )}
    </section>
  );

  const getStatusBadge = (status: string) => {
    switch (status) {
      case "queued":
        return <span className="rounded bg-gray-500/10 px-2 py-0.5 text-xs text-gray-400 font-medium">Queued</span>;
      case "running":
        return (
          <span className="flex items-center gap-1 rounded bg-blue-500/10 px-2 py-0.5 text-xs text-blue-400 font-medium animate-pulse">
            <span className="h-1.5 w-1.5 rounded-full bg-blue-400 animate-ping" />
            Running
          </span>
        );
      case "waiting_approval":
        return (
          <span className="flex items-center gap-1 rounded bg-amber-500/10 px-2 py-0.5 text-xs text-amber-400 font-medium">
            <ShieldAlert className="h-3.5 w-3.5" />
            Waiting Approval
          </span>
        );
      case "completed":
        return <span className="rounded bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-400 font-medium">Completed</span>;
      case "failed":
        return <span className="rounded bg-rose-500/10 px-2 py-0.5 text-xs text-rose-400 font-medium">Failed</span>;
      case "cancelled":
        return <span className="rounded bg-slate-500/10 px-2 py-0.5 text-xs text-slate-400 font-medium">Cancelled</span>;
      default:
        return <span className="rounded bg-gray-500/10 px-2 py-0.5 text-xs text-gray-400 font-medium">{status}</span>;
    }
  };

  const handleInterrupt = async () => {
    if (!turn) return;
    setCancelling(true);
    try {
      await cancelTurn(turn.id);
    } catch (err) {
      console.error("Failed to cancel turn:", err);
    } finally {
      setCancelling(false);
    }
  };

  const renderTurnInspector = () => {
    if (!turn) {
      return (
        <section className="flex flex-col items-center justify-center gap-2 py-10 text-sm text-gray-500">
          <Activity className="h-8 w-8 text-gray-700 animate-pulse" />
          <span>Select or start a turn to inspect</span>
        </section>
      );
    }

    const pendingApproval = turn.approvals.find((approval) => approval.status === "pending");

    return (
      <>
        <section className="border-b border-white/10 pb-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
              <Zap className="h-4.5 w-4.5 text-primary" />
              Active Turn Inspector
            </div>
            {getStatusBadge(turn.status)}
          </div>
          <div className="mt-3 space-y-1.5 text-xs text-gray-400 bg-white/[0.02] border border-white/5 rounded-md p-3">
            <div className="flex justify-between">
              <span className="text-gray-500">ID</span>
              <span className="font-mono text-gray-300">{turn.id.slice(0, 8)}...</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-500">Backend</span>
              <span className="font-medium text-gray-300 uppercase">{turn.backend}</span>
            </div>
            {(turn.status === "running" || turn.status === "waiting_approval") && (
              <div className="mt-2 pt-2 border-t border-white/5 flex justify-end">
                <button
                  disabled={cancelling}
                  onClick={handleInterrupt}
                  className="w-full flex items-center justify-center gap-1.5 rounded bg-rose-500/10 hover:bg-rose-500/20 text-rose-300 disabled:opacity-50 py-1.5 text-xs font-semibold transition-colors"
                >
                  <Ban className="h-3.5 w-3.5" />
                  {cancelling ? "Interrupting..." : "Interrupt Execution"}
                </button>
              </div>
            )}
          </div>
          <div className="mt-3">
            <button
              onClick={() => setActiveTab("memory")}
              className="w-full flex items-center justify-center gap-1.5 rounded border border-white/10 bg-white/[0.02] hover:bg-white/[0.06] text-gray-300 py-1.5 text-xs font-semibold transition-colors cursor-pointer"
            >
              <Cpu className="h-3.5 w-3.5 text-primary" />
              Open Memory
            </button>
          </div>
        </section>

        {pendingApproval && (
          <section className="rounded-md border border-rose-500/30 bg-rose-500/10 p-4 animate-pulse">
            <div className="flex items-center gap-2 text-sm font-semibold text-rose-200">
              <AlertCircle className="h-4.5 w-4.5" />
              Approval Required
            </div>
            <p className="mt-2 text-xs text-gray-300">
              The agent requested execution of tool: <strong className="font-mono text-white">{pendingApproval.toolName}</strong>
            </p>
            <pre className="mt-2 max-h-36 overflow-auto rounded bg-black/40 p-2.5 text-[11px] text-gray-400 font-mono">
              {JSON.stringify(pendingApproval.params, null, 2)}
            </pre>
            <div className="mt-4 flex justify-end gap-2">
              <button
                className="rounded border border-white/10 px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10 transition-colors"
                onClick={async () => {
                  await handleApproval(pendingApproval.id, false);
                  onApprovalDecision(pendingApproval.id, false);
                }}
              >
                Deny
              </button>
              <button
                className="rounded bg-primary px-4 py-1.5 text-xs text-white font-semibold hover:bg-primary/95 transition-colors"
                onClick={async () => {
                  await handleApproval(pendingApproval.id, true);
                  onApprovalDecision(pendingApproval.id, true);
                }}
              >
                Approve
              </button>
            </div>
          </section>
        )}

        <section className="bg-white/[0.01] border border-white/5 rounded-md p-4">
          <div className="flex items-center gap-2 text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">
            <Clock className="h-4 w-4 text-primary" />
            Timing Summary
          </div>
          {turn.timing ? (
            <div className="grid grid-cols-2 gap-3 text-xs">
              <div className="bg-white/[0.02] p-2.5 rounded border border-white/5">
                <div className="text-gray-500">Total Duration</div>
                <div className="text-sm font-semibold text-gray-200 mt-0.5">{formatMs(turn.timing.total_ms)}</div>
              </div>
              <div className="bg-white/[0.02] p-2.5 rounded border border-white/5">
                <div className="text-gray-500">LLM Prompt</div>
                <div className="text-sm font-semibold text-gray-200 mt-0.5">{formatMs(turn.timing.prompt_ms)}</div>
              </div>
              <div className="bg-white/[0.02] p-2.5 rounded border border-white/5">
                <div className="text-gray-500">Spawn / Init</div>
                <div className="text-sm font-semibold text-gray-300 mt-0.5">
                  {formatMs(turn.timing.process_spawn_ms)} / {formatMs(turn.timing.init_ms)}
                </div>
              </div>
              <div className="bg-white/[0.02] p-2.5 rounded border border-white/5">
                <div className="text-gray-500">Session Mode</div>
                <div className="mt-1">
                  {turn.timing.session_reused ? (
                    <span className="rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-400 font-medium">Reused</span>
                  ) : (
                    <span className="rounded bg-blue-500/10 px-1.5 py-0.5 text-[10px] text-blue-400 font-medium">Cold Start</span>
                  )}
                </div>
              </div>
            </div>
          ) : (
            <div className="text-xs text-gray-600 italic">No timing recorded yet</div>
          )}
        </section>

        <section className="bg-white/[0.01] border border-white/5 rounded-md p-4">
          <div className="flex items-center gap-2 text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">
            <Coins className="h-4 w-4 text-primary" />
            Token Usage
          </div>
          {turn.usage ? (
            <div className="space-y-2.5 text-xs text-gray-300">
              <div className="flex justify-between items-center">
                <span className="text-gray-500">Input Tokens</span>
                <span className="font-semibold">{formatNumber(turn.usage.input_tokens)}</span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-gray-500">Output Tokens</span>
                <span className="font-semibold">{formatNumber(turn.usage.output_tokens)}</span>
              </div>
              {turn.usage.thinking_tokens > 0 && (
                <div className="flex justify-between items-center">
                  <span className="text-gray-500">Thinking (Reasoning)</span>
                  <span className="font-semibold text-gray-400">{formatNumber(turn.usage.thinking_tokens)}</span>
                </div>
              )}
              {turn.usage.cache_read_input_tokens > 0 && (
                <div className="flex justify-between items-center">
                  <span className="text-gray-500">Cache Read Input</span>
                  <span className="font-semibold text-emerald-500">{formatNumber(turn.usage.cache_read_input_tokens)}</span>
                </div>
              )}
              <div className="pt-2 border-t border-white/5 flex justify-between items-center text-sm">
                <span className="font-medium text-gray-400">Total Tokens</span>
                <span className="font-bold text-primary">{formatNumber(turn.usage.total_tokens)}</span>
              </div>
            </div>
          ) : (
            <div className="text-xs text-gray-600 italic">No usage recorded yet</div>
          )}
        </section>

        <section className="flex flex-col gap-2">
          <div className="flex items-center gap-2 text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">
            <Terminal className="h-4 w-4 text-primary" />
            Tool Execution ({turn.toolCalls.length})
          </div>
          <div className="space-y-2.5 max-h-72 overflow-y-auto pr-1">
            {turn.toolCalls.length === 0 ? (
              <div className="text-xs text-gray-600 italic">No tools invoked in this turn</div>
            ) : null}
            {turn.toolCalls.map((tool) => (
              <div key={tool.id} className="rounded-md border border-white/5 bg-white/[0.02] overflow-hidden">
                <div className="flex items-center justify-between bg-white/[0.02] border-b border-white/5 px-3 py-2 text-xs">
                  <div className="flex items-center gap-1.5 font-medium text-gray-200">
                    <Cpu className="h-3.5 w-3.5 text-primary" />
                    {tool.name}
                  </div>
                  {tool.ok === true ? (
                    <span className="flex items-center gap-1 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[10px] text-emerald-400 font-semibold">
                      <CheckCircle2 className="h-3 w-3" /> OK
                    </span>
                  ) : tool.ok === false ? (
                    <span className="flex items-center gap-1 rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-400 font-semibold">
                      <AlertCircle className="h-3 w-3" /> Error
                    </span>
                  ) : (
                    <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-400 font-medium">Running</span>
                  )}
                </div>
                <details className="text-xs text-gray-400">
                  <summary className="px-3 py-1.5 cursor-pointer text-[10px] text-gray-500 hover:text-gray-300 select-none">
                    Parameters & Results
                  </summary>
                  <div className="p-3 bg-black/20 space-y-2.5 font-mono">
                    <div>
                      <div className="text-[10px] text-gray-500 mb-1">Arguments</div>
                      <pre className="text-[11px] bg-black/40 rounded p-2 overflow-x-auto text-gray-300">
                        {JSON.stringify(tool.arguments, null, 2)}
                      </pre>
                    </div>
                    {tool.result !== undefined && (
                      <div>
                        <div className="text-[10px] text-gray-500 mb-1">Result</div>
                        <pre className="text-[11px] bg-black/40 rounded p-2 overflow-x-auto text-gray-300 max-h-36 overflow-y-auto">
                          {JSON.stringify(tool.result, null, 2)}
                        </pre>
                      </div>
                    )}
                  </div>
                </details>
              </div>
            ))}
          </div>
        </section>

        <section className="flex flex-col gap-2">
          <div className="flex items-center gap-2 text-xs font-semibold text-gray-400 uppercase tracking-wider mb-1">
            <Activity className="h-4 w-4 text-primary" />
            Runtime Events ({turn.events.length})
          </div>
          <div className="space-y-2 max-h-72 overflow-y-auto pr-1">
            {turn.events.length === 0 ? (
              <div className="text-xs text-gray-600 italic">No events generated yet</div>
            ) : null}
            {turn.events.map((event, index) => (
              <details key={index} className="rounded border border-white/5 bg-white/[0.01] text-xs">
                <summary className="px-3 py-2 cursor-pointer font-medium text-gray-300 hover:bg-white/[0.02] flex justify-between items-center select-none">
                  <span>{event.kind}</span>
                  <span className="text-[10px] text-gray-600 font-mono">#{index + 1}</span>
                </summary>
                <pre className="p-3 bg-black/30 border-t border-white/5 font-mono text-[10px] text-gray-400 overflow-x-auto max-h-48 overflow-y-auto">
                  {JSON.stringify(event.data, null, 2)}
                </pre>
              </details>
            ))}
          </div>
        </section>
      </>
    );
  };

  return (
    <aside
      className="shrink-0 border-l border-white/10 bg-[#070a13] overflow-hidden flex flex-col"
      style={{ width }}
    >
      <div className="border-b border-white/10 bg-[#070a13] p-3">
        <nav className="grid grid-cols-3 rounded-md border border-white/5 bg-white/[0.03] p-1">
          <button
            className={`flex items-center justify-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold transition-all ${
              activeTab === "observability" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
            }`}
            onClick={() => setActiveTab("observability")}
          >
            <Activity className="h-3.5 w-3.5" />
            Observability
          </button>
          <button
            className={`flex items-center justify-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold transition-all ${
              activeTab === "memory" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
            }`}
            onClick={() => setActiveTab("memory")}
          >
            <Database className="h-3.5 w-3.5" />
            Memory
          </button>
          <button
            className={`flex items-center justify-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold transition-all ${
              activeTab === "context" ? "bg-primary text-white shadow" : "text-gray-400 hover:text-white"
            }`}
            onClick={() => setActiveTab("context")}
          >
            <Cpu className="h-3.5 w-3.5" />
            Context
          </button>
        </nav>
      </div>
      {activeTab === "observability" ? (
        <div className="flex flex-col gap-6 overflow-y-auto p-5">
          {renderTurnInspector()}
          {renderObservabilitySection()}
        </div>
      ) : activeTab === "memory" ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <MemoryContextWorkspace mode="memory" />
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-hidden">
          <MemoryContextWorkspace mode="context" />
        </div>
      )}
    </aside>
  );
}
