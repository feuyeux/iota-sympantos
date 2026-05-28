import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, AlertCircle, Link2, MessageSquare, Play, RefreshCw, X } from "lucide-react";
import { dispatchKanban, getKanbanTaskDetail, listenKanbanUpdates, listKanbanBoards, listKanbanTasks } from "../api";
import type { KanbanBoard, KanbanDispatchReport, KanbanEvent, KanbanStatus, KanbanTask, KanbanTaskDetail } from "../types";

const STATUSES: KanbanStatus[] = ["triage", "todo", "ready", "running", "blocked", "done", "archived"];

const STATUS_LABELS: Record<KanbanStatus, string> = {
  triage: "Triage",
  todo: "Todo",
  ready: "Ready",
  running: "Running",
  blocked: "Blocked",
  done: "Done",
  archived: "Archived",
};

const STATUS_STYLES: Record<KanbanStatus, string> = {
  triage: "border-slate-700/70 bg-slate-900/30 text-slate-300",
  todo: "border-sky-500/25 bg-sky-500/10 text-sky-200",
  ready: "border-amber-500/25 bg-amber-500/10 text-amber-200",
  running: "border-blue-500/25 bg-blue-500/10 text-blue-200",
  blocked: "border-rose-500/25 bg-rose-500/10 text-rose-200",
  done: "border-emerald-500/25 bg-emerald-500/10 text-emerald-200",
  archived: "border-slate-700 bg-slate-950/30 text-slate-500",
};

function formatBoardName(task: KanbanTask, boards: KanbanBoard[]) {
  return boards.find((board) => board.id === task.board_id)?.slug ?? `board-${task.board_id}`;
}

function formatDispatchReport(report?: KanbanDispatchReport | null) {
  if (!report) return "Auto-dispatch is active";
  const parts = [
    report.spawned ? `${report.spawned} spawned` : undefined,
    report.completed ? `${report.completed} completed` : undefined,
    report.reclaimed ? `${report.reclaimed} reclaimed` : undefined,
    report.timed_out ? `${report.timed_out} timed out` : undefined,
    report.spawn_failures ? `${report.spawn_failures} failed` : undefined,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : `${report.active_workers} active workers`;
}

function formatUnixSeconds(value?: number) {
  if (!value) return "-";
  return new Date(value * 1000).toLocaleString();
}

function formatEventPayload(event: KanbanEvent) {
  try {
    return JSON.stringify(JSON.parse(event.payload), null, 2);
  } catch {
    return event.payload;
  }
}

export function KanbanWorkspace() {
  const [boards, setBoards] = useState<KanbanBoard[]>([]);
  const [tasks, setTasks] = useState<KanbanTask[]>([]);
  const [selectedStatus, setSelectedStatus] = useState<KanbanStatus | "all">("all");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [dispatching, setDispatching] = useState(false);
  const [lastReport, setLastReport] = useState<KanbanDispatchReport | null>(null);
  const [selectedTaskId, setSelectedTaskId] = useState<number | null>(null);
  const [detail, setDetail] = useState<KanbanTaskDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (options: { tick?: boolean; showSpinner?: boolean } = {}) => {
    if (options.showSpinner) {
      setRefreshing(true);
      setLoading(true);
    }
    try {
      setError(null);
      if (options.tick) {
        setLastReport(await dispatchKanban());
      }
      const [nextBoards, nextTasks] = await Promise.all([listKanbanBoards(), listKanbanTasks({ limit: 200 })]);
      setBoards(nextBoards);
      setTasks(nextTasks);
      if (selectedTaskId !== null) {
        setDetail(await getKanbanTaskDetail(selectedTaskId));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [selectedTaskId]);

  useEffect(() => {
    refresh();
    const interval = window.setInterval(refresh, 3000);
    return () => window.clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    listenKanbanUpdates((report) => {
      if (disposed) return;
      setLastReport(report);
      refresh();
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          cleanup = unlisten;
        }
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [refresh]);

  useEffect(() => {
    if (selectedTaskId === null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    setDetailLoading(true);
    getKanbanTaskDetail(selectedTaskId)
      .then((nextDetail) => {
        if (!cancelled) setDetail(nextDetail);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTaskId]);

  const counts = useMemo(() => {
    const next = Object.fromEntries(STATUSES.map((status) => [status, 0])) as Record<KanbanStatus, number>;
    for (const task of tasks) next[task.status] += 1;
    return next;
  }, [tasks]);

  const visibleTasks = useMemo(() => {
    if (selectedStatus === "all") return tasks;
    return tasks.filter((task) => task.status === selectedStatus);
  }, [selectedStatus, tasks]);

  const runDispatch = async () => {
    setDispatching(true);
    try {
      setLastReport(await dispatchKanban());
      await refresh({ showSpinner: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setDispatching(false);
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#0d1220]">
      <div className="border-b border-slate-800/80 p-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="flex items-center gap-2 text-[13px] font-bold text-slate-200">
              <Activity className="h-4 w-4 text-primary" />
              Hermes Kanban
            </div>
            <p className="mt-1 text-[11px] text-slate-500">{formatDispatchReport(lastReport)}</p>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => refresh({ tick: true, showSpinner: true })}
              disabled={refreshing}
              className="flex h-8 w-8 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/25 text-slate-300 transition-colors hover:bg-slate-950/45"
              aria-label="Refresh kanban tasks"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading || refreshing ? "animate-spin" : ""}`} />
            </button>
            <button
              type="button"
              onClick={runDispatch}
              disabled={dispatching}
              className="flex h-8 items-center gap-1.5 rounded-lg bg-primary px-3 text-xs font-semibold text-white shadow-sm shadow-primary/20 transition-colors hover:bg-primary-hover disabled:opacity-50"
            >
              <Play className="h-3.5 w-3.5" />
              Dispatch
            </button>
          </div>
        </div>

        <div className="mt-4 flex gap-2 overflow-x-auto pb-1">
          <button
            type="button"
            onClick={() => setSelectedStatus("all")}
            className={`shrink-0 rounded-lg border px-3 py-1.5 text-[11px] font-semibold transition-colors ${
              selectedStatus === "all"
                ? "border-primary/50 bg-primary/15 text-white"
                : "border-slate-800 bg-slate-950/20 text-slate-400 hover:text-slate-200"
            }`}
          >
            All {tasks.length}
          </button>
          {STATUSES.map((status) => (
            <button
              key={status}
              type="button"
              onClick={() => setSelectedStatus(status)}
              className={`shrink-0 rounded-lg border px-3 py-1.5 text-[11px] font-semibold transition-colors ${
                selectedStatus === status
                  ? "border-primary/50 bg-primary/15 text-white"
                  : "border-slate-800 bg-slate-950/20 text-slate-400 hover:text-slate-200"
              }`}
            >
              {STATUS_LABELS[status]} {counts[status]}
            </button>
          ))}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {error ? (
          <div className="mb-3 flex items-start gap-2 rounded-xl border border-rose-500/25 bg-rose-500/10 p-3 text-xs text-rose-200">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{error}</span>
          </div>
        ) : null}
        {loading && tasks.length === 0 ? (
          <div className="rounded-xl border border-dashed border-slate-800 bg-slate-950/10 p-8 text-center text-xs text-slate-500">
            Loading kanban tasks...
          </div>
        ) : visibleTasks.length === 0 ? (
          <div className="rounded-xl border border-dashed border-slate-800 bg-slate-950/10 p-8 text-center text-xs text-slate-500">
            No tasks in this view
          </div>
        ) : (
          <div className="space-y-3">
            {selectedTaskId !== null ? (
              <section className="rounded-xl border border-primary/30 bg-primary/5 p-3.5 shadow-sm shadow-primary/5">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="text-[10px] font-bold uppercase tracking-wider text-primary/90">Task Detail</div>
                    <h3 className="mt-1 truncate text-sm font-bold text-slate-100">
                      {detail?.task.title ?? `#${selectedTaskId}`}
                    </h3>
                    <p className="mt-1 text-[11px] text-slate-500">
                      {detailLoading ? "Loading detail..." : detail?.board?.slug ?? "iota kanban db"}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => setSelectedTaskId(null)}
                    className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/25 text-slate-400 hover:text-slate-100"
                    aria-label="Close task detail"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>

                {detail ? (
                  <div className="mt-3 space-y-3">
                    {detail.task.body ? <p className="text-xs leading-relaxed text-slate-300">{detail.task.body}</p> : null}
                    <div className="grid grid-cols-2 gap-2 text-[11px] text-slate-400">
                      <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2">
                        <span className="block text-slate-600">Created</span>
                        <span className="font-mono text-slate-300">{formatUnixSeconds(detail.task.created_at)}</span>
                      </div>
                      <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2">
                        <span className="block text-slate-600">Updated</span>
                        <span className="font-mono text-slate-300">{formatUnixSeconds(detail.task.updated_at)}</span>
                      </div>
                    </div>

                    <div className="grid grid-cols-3 gap-2 text-[11px]">
                      <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                        <span className="block text-slate-600">Runs</span>
                        {detail.runs.length}
                      </div>
                      <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                        <span className="block text-slate-600">Comments</span>
                        {detail.comments.length}
                      </div>
                      <div className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-slate-300">
                        <span className="block text-slate-600">Links</span>
                        {detail.links.length}
                      </div>
                    </div>

                    <div className="space-y-2">
                      <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                        <Activity className="h-3 w-3 text-primary/80" /> Runs
                      </div>
                      {detail.runs.length === 0 ? (
                        <div className="text-xs text-slate-500">No runs recorded</div>
                      ) : detail.runs.slice(0, 3).map((run) => (
                        <div key={run.id} className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-[11px] text-slate-400">
                          <div className="flex items-center justify-between gap-2">
                            <span className="truncate font-mono text-slate-300">{run.id}</span>
                            <span className="shrink-0 rounded border border-slate-700 px-1.5 py-0.5 text-[10px] uppercase text-slate-300">{run.status}</span>
                          </div>
                          {run.output_summary ? <p className="mt-1 text-slate-400">{run.output_summary}</p> : null}
                        </div>
                      ))}
                    </div>

                    <div className="space-y-2">
                      <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                        <MessageSquare className="h-3 w-3 text-primary/80" /> Comments
                      </div>
                      {detail.comments.length === 0 ? (
                        <div className="text-xs text-slate-500">No comments recorded</div>
                      ) : detail.comments.slice(0, 3).map((comment) => (
                        <div key={comment.id} className="rounded-lg border border-slate-800 bg-slate-950/25 p-2 text-[11px] text-slate-400">
                          <div className="font-semibold text-slate-300">{comment.author}</div>
                          <p className="mt-1 leading-relaxed">{comment.body}</p>
                        </div>
                      ))}
                    </div>

                    <div className="space-y-2">
                      <div className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-wider text-slate-500">
                        <Link2 className="h-3 w-3 text-primary/80" /> Links & Events
                      </div>
                      {detail.links.length > 0 ? (
                        <div className="flex flex-wrap gap-1.5">
                          {detail.links.map((link) => (
                            <span key={`${link.from_id}-${link.to_id}-${link.kind}`} className="rounded border border-slate-800 bg-slate-950/25 px-2 py-0.5 text-[10px] text-slate-400">
                              #{link.from_id} {link.kind} #{link.to_id}
                            </span>
                          ))}
                        </div>
                      ) : null}
                      {detail.events.length === 0 ? (
                        <div className="text-xs text-slate-500">No related events recorded</div>
                      ) : detail.events.slice(0, 4).map((event) => (
                        <details key={event.id} className="rounded-lg border border-slate-800 bg-slate-950/25 text-[11px] text-slate-400">
                          <summary className="cursor-pointer px-2 py-1.5 font-semibold text-slate-300">{event.event_type}</summary>
                          <pre className="max-h-32 overflow-auto border-t border-slate-800 p-2 font-mono text-[10px] text-slate-500">{formatEventPayload(event)}</pre>
                        </details>
                      ))}
                    </div>
                  </div>
                ) : detailLoading ? null : (
                  <div className="mt-3 text-xs text-slate-500">Select another task or refresh the board.</div>
                )}
              </section>
            ) : null}
            {visibleTasks.map((task) => (
              <article
                key={task.id}
                onClick={() => setSelectedTaskId(task.id)}
                className={`rounded-xl border bg-slate-950/20 p-3.5 shadow-sm transition-colors cursor-pointer ${
                  selectedTaskId === task.id ? "border-primary/40 ring-1 ring-primary/25" : "border-slate-800/80 hover:border-slate-700"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2 text-[11px] font-mono text-slate-500">
                      <span>#{task.id}</span>
                      <span>{formatBoardName(task, boards)}</span>
                    </div>
                    <h3 className="mt-1 text-sm font-bold leading-snug text-slate-100">{task.title}</h3>
                  </div>
                  <span className={`shrink-0 rounded-md border px-2 py-1 text-[10px] font-bold ${STATUS_STYLES[task.status]}`}>
                    {STATUS_LABELS[task.status]}
                  </span>
                </div>
                {task.body ? <p className="mt-2 line-clamp-3 text-xs leading-relaxed text-slate-400">{task.body}</p> : null}
                <div className="mt-3 flex flex-wrap items-center gap-2 text-[10px] text-slate-500">
                  {task.assignee ? <span className="rounded border border-slate-800 bg-slate-950/30 px-2 py-0.5">{task.assignee}</span> : null}
                  <span className="rounded border border-slate-800 bg-slate-950/30 px-2 py-0.5">P{task.priority}</span>
                  {task.tags.slice(0, 4).map((tag) => (
                    <span key={tag} className="rounded border border-slate-800 bg-slate-950/30 px-2 py-0.5">{tag}</span>
                  ))}
                </div>
              </article>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}