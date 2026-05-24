import { AlertCircle, CheckCircle2, Clock, Terminal, Zap } from "lucide-react";
import type { DesktopTurn } from "../types";
import { handleApproval } from "../api";

type Props = {
  turn?: DesktopTurn;
  onApprovalDecision: (approvalId: string, approved: boolean) => void;
};

export function RightInspector({ turn, onApprovalDecision }: Props) {
  if (!turn) {
    return (
      <aside className="w-[360px] border-l border-white/10 bg-[#070a13] p-4 text-sm text-gray-500">
        No active turn
      </aside>
    );
  }

  const pendingApproval = turn.approvals.find((approval) => approval.status === "pending");

  return (
    <aside className="w-[360px] border-l border-white/10 bg-[#070a13] p-4 overflow-y-auto">
      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Zap className="h-4 w-4 text-primary" />
          Turn Status
        </div>
        <div className="mt-2 rounded-md border border-white/10 bg-white/[0.03] p-3 text-xs text-gray-300">
          <div className="flex justify-between"><span>Status</span><span>{turn.status}</span></div>
          <div className="flex justify-between"><span>Backend</span><span>{turn.backend}</span></div>
          <div className="truncate text-gray-500" title={turn.cwd}>{turn.cwd}</div>
        </div>
      </section>

      {pendingApproval && (
        <section className="mb-5 rounded-md border border-rose-500/30 bg-rose-500/10 p-3">
          <div className="flex items-center gap-2 text-sm font-semibold text-rose-200">
            <AlertCircle className="h-4 w-4" />
            Approval Required
          </div>
          <div className="mt-2 text-xs text-gray-300">{pendingApproval.toolName}</div>
          <pre className="mt-2 max-h-36 overflow-auto rounded bg-black/40 p-2 text-[11px] text-gray-400">
            {JSON.stringify(pendingApproval.params, null, 2)}
          </pre>
          <div className="mt-3 flex justify-end gap-2">
            <button
              className="rounded border border-white/10 px-3 py-1.5 text-xs text-gray-300 hover:bg-white/10"
              onClick={async () => {
                await handleApproval(pendingApproval.id, false);
                onApprovalDecision(pendingApproval.id, false);
              }}
            >
              Deny
            </button>
            <button
              className="rounded bg-primary px-3 py-1.5 text-xs text-white"
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

      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Clock className="h-4 w-4 text-primary" />
          Timing & Usage
        </div>
        <pre className="mt-2 max-h-36 overflow-auto rounded-md border border-white/10 bg-white/[0.03] p-3 text-[11px] text-gray-400">
          {JSON.stringify({ timing: turn.timing, usage: turn.usage }, null, 2)}
        </pre>
      </section>

      <section className="mb-5">
        <div className="flex items-center gap-2 text-sm font-semibold text-gray-200">
          <Terminal className="h-4 w-4 text-primary" />
          Tool Calls
        </div>
        <div className="mt-2 space-y-2">
          {turn.toolCalls.length === 0 ? <div className="text-xs text-gray-600">No tool calls</div> : null}
          {turn.toolCalls.map((tool) => (
            <div key={tool.id} className="rounded-md border border-white/10 bg-white/[0.03] p-2 text-xs text-gray-300">
              <div className="flex items-center justify-between">
                <span>{tool.name}</span>
                {tool.ok === true ? <CheckCircle2 className="h-3.5 w-3.5 text-emerald-400" /> : null}
              </div>
              <pre className="mt-1 max-h-24 overflow-auto text-[10px] text-gray-500">
                {JSON.stringify(tool.arguments, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </section>

      <section>
        <div className="text-sm font-semibold text-gray-200">Runtime Events</div>
        <div className="mt-2 space-y-1">
          {turn.events.map((event, index) => (
            <details key={index} className="rounded border border-white/10 bg-white/[0.03] p-2 text-xs text-gray-400">
              <summary>{event.kind}</summary>
              <pre className="mt-2 max-h-32 overflow-auto text-[10px]">{JSON.stringify(event.data, null, 2)}</pre>
            </details>
          ))}
        </div>
      </section>
    </aside>
  );
}
