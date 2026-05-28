# Hermes Kanban Desktop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the approved "Official-Feeling MVP" for desktop Kanban: six-column board, profile lanes, filters, drawer-style task details, logs, comments, and basic task operations.

**Architecture:** Keep `iota-kanban` as the source of truth and expose only small Tauri command additions from `src-tauri/src/lib.rs`. Move frontend Kanban logic into testable helper functions, then make `KanbanWorkspace.tsx` a board-and-drawer UI that consumes those helpers.

**Tech Stack:** Rust/Tauri commands, existing `iota-kanban` store trait, React 19, TypeScript, lucide-react, node test runner with `tsx`, `cargo check`.

---

## File Structure

- Modify `crates/iota-desktop/src-tauri/src/lib.rs`: add task update and link commands; keep log tail support in task detail.
- Modify `crates/iota-desktop/src/api.ts`: add wrappers for new Tauri commands.
- Modify `crates/iota-desktop/src/types.ts`: add frontend request types for task patch and link operations.
- Create `crates/iota-desktop/src/components/kanbanModel.ts`: pure helpers for grouping, filtering, lanes, legal actions, and dispatch report text.
- Create `crates/iota-desktop/src/components/kanbanModel.test.ts`: unit tests for the pure helpers.
- Modify `crates/iota-desktop/src/components/KanbanWorkspace.tsx`: replace list UI with board columns, filters, lanes, drawer, comments, and actions.
- Modify `crates/iota-desktop/src/components/layout_structure.test.ts`: assert Kanban stays in right inspector if needed.

## Task 1: Extract Testable Kanban Model Helpers

**Files:**
- Create: `crates/iota-desktop/src/components/kanbanModel.ts`
- Create: `crates/iota-desktop/src/components/kanbanModel.test.ts`
- Modify: `crates/iota-desktop/src/components/KanbanWorkspace.tsx`

- [ ] **Step 1: Write failing helper tests**

Create `crates/iota-desktop/src/components/kanbanModel.test.ts`:

```ts
import test from "node:test";
import assert from "node:assert/strict";
import { buildKanbanColumns, filterKanbanTasks, formatDispatchReport, groupRunningByLane, legalStatusActions } from "./kanbanModel";
import type { KanbanTask } from "../types";

function task(id: number, status: KanbanTask["status"], overrides: Partial<KanbanTask> = {}): KanbanTask {
  return {
    id,
    board_id: 1,
    title: `Task ${id}`,
    body: "",
    status,
    priority: 0,
    tags: [],
    created_at: 1,
    updated_at: 1,
    claim_ttl_secs: 0,
    ...overrides,
  };
}

test("buildKanbanColumns returns all six visible board columns in order", () => {
  const columns = buildKanbanColumns([task(1, "ready"), task(2, "running"), task(3, "archived")]);
  assert.deepEqual(columns.map((column) => column.status), ["triage", "todo", "ready", "running", "blocked", "done"]);
  assert.deepEqual(columns.find((column) => column.status === "ready")?.tasks.map((item) => item.id), [1]);
  assert.equal(columns.some((column) => column.tasks.some((item) => item.status === "archived")), false);
});

test("filterKanbanTasks matches search, board, and assignee", () => {
  const tasks = [
    task(1, "ready", { title: "Implement API", assignee: "backend-dev", tags: ["auth"] }),
    task(2, "ready", { title: "Review copy", assignee: "reviewer", tags: ["docs"] }),
  ];
  assert.deepEqual(filterKanbanTasks(tasks, { search: "auth", assignee: "backend-dev", boardId: 1 }).map((item) => item.id), [1]);
  assert.deepEqual(filterKanbanTasks(tasks, { search: "missing", assignee: "all", boardId: "all" }), []);
});

test("groupRunningByLane groups missing assignee under default", () => {
  const groups = groupRunningByLane([task(1, "running", { assignee: "backend-dev" }), task(2, "running")]);
  assert.deepEqual(groups.map((group) => group.lane), ["backend-dev", "default"]);
  assert.deepEqual(groups.map((group) => group.tasks.length), [1, 1]);
});

test("legalStatusActions only exposes valid transitions", () => {
  assert.deepEqual(legalStatusActions("blocked").map((action) => action.to), ["ready", "done"]);
  assert.deepEqual(legalStatusActions("done").map((action) => action.to), ["archived"]);
  assert.deepEqual(legalStatusActions("ready"), []);
});

test("formatDispatchReport makes a no-op nudge visible", () => {
  assert.equal(
    formatDispatchReport({ spawned: 0, completed: 0, timed_out: 0, spawn_failures: 0, reclaimed: 0, active_workers: 1 }),
    "No ready tasks · 1 active worker",
  );
});
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cd crates/iota-desktop && npm test -- src/components/kanbanModel.test.ts
```

Expected: FAIL because `./kanbanModel` does not exist.

- [ ] **Step 3: Implement helper module**

Create `crates/iota-desktop/src/components/kanbanModel.ts`:

```ts
import type { KanbanDispatchReport, KanbanStatus, KanbanTask } from "../types";

export const BOARD_STATUSES = ["triage", "todo", "ready", "running", "blocked", "done"] as const satisfies readonly KanbanStatus[];

export const STATUS_LABELS: Record<KanbanStatus, string> = {
  triage: "Triage",
  todo: "Todo",
  ready: "Ready",
  running: "Running",
  blocked: "Blocked",
  done: "Done",
  archived: "Archived",
};

export type KanbanFilters = {
  search: string;
  assignee: string | "all";
  boardId: number | "all";
};

export function formatDispatchReport(report?: KanbanDispatchReport | null) {
  if (!report) return "Auto-dispatch is active";
  const parts = [
    report.spawned ? `${report.spawned} spawned` : undefined,
    report.completed ? `${report.completed} completed` : undefined,
    report.reclaimed ? `${report.reclaimed} reclaimed` : undefined,
    report.timed_out ? `${report.timed_out} timed out` : undefined,
    report.spawn_failures ? `${report.spawn_failures} failed` : undefined,
  ].filter(Boolean);
  const workerLabel = report.active_workers === 1 ? "active worker" : "active workers";
  return parts.length > 0 ? parts.join(" · ") : `No ready tasks · ${report.active_workers} ${workerLabel}`;
}

export function filterKanbanTasks(tasks: KanbanTask[], filters: KanbanFilters) {
  const needle = filters.search.trim().toLowerCase();
  return tasks.filter((task) => {
    if (filters.boardId !== "all" && task.board_id !== filters.boardId) return false;
    if (filters.assignee !== "all" && (task.assignee ?? "default") !== filters.assignee) return false;
    if (!needle) return true;
    const haystack = [task.title, task.body ?? "", task.assignee ?? "", ...task.tags].join(" ").toLowerCase();
    return haystack.includes(needle);
  });
}

export function buildKanbanColumns(tasks: KanbanTask[]) {
  return BOARD_STATUSES.map((status) => ({
    status,
    label: STATUS_LABELS[status],
    tasks: tasks.filter((task) => task.status === status),
  }));
}

export function groupRunningByLane(tasks: KanbanTask[]) {
  const map = new Map<string, KanbanTask[]>();
  for (const task of tasks.filter((item) => item.status === "running")) {
    const lane = task.assignee || "default";
    map.set(lane, [...(map.get(lane) ?? []), task]);
  }
  return [...map.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([lane, laneTasks]) => ({ lane, tasks: laneTasks }));
}

export function uniqueAssignees(tasks: KanbanTask[]) {
  return [...new Set(tasks.map((task) => task.assignee || "default"))].sort((left, right) => left.localeCompare(right));
}

export function legalStatusActions(status: KanbanStatus): Array<{ label: string; to: KanbanStatus }> {
  switch (status) {
    case "triage":
      return [{ label: "Move to Todo", to: "todo" }];
    case "todo":
      return [{ label: "Move to Ready", to: "ready" }];
    case "running":
      return [
        { label: "Mark Done", to: "done" },
        { label: "Block", to: "blocked" },
        { label: "Requeue", to: "ready" },
      ];
    case "blocked":
      return [
        { label: "Unblock", to: "ready" },
        { label: "Mark Done", to: "done" },
      ];
    case "done":
      return [{ label: "Archive", to: "archived" }];
    default:
      return [];
  }
}
```

- [ ] **Step 4: Update `KanbanWorkspace.tsx` imports**

Remove local `STATUSES`, `STATUS_LABELS`, and `formatDispatchReport`; import them from `kanbanModel.ts`.

- [ ] **Step 5: Run tests**

Run:

```bash
cd crates/iota-desktop && npm test -- src/components/kanbanModel.test.ts
```

Expected: PASS for all helper tests.

## Task 2: Add Missing Desktop Kanban Commands

**Files:**
- Modify: `crates/iota-desktop/src-tauri/src/lib.rs`
- Modify: `crates/iota-desktop/src/api.ts`
- Modify: `crates/iota-desktop/src/types.ts`

- [ ] **Step 1: Add frontend types**

In `crates/iota-desktop/src/types.ts`, add:

```ts
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

export type KanbanCreateLinkRequest = {
  from_id: number;
  to_id: number;
  kind: KanbanLinkKind;
};
```

- [ ] **Step 2: Add API wrappers**

In `crates/iota-desktop/src/api.ts`, import `KanbanCreateLinkRequest` and `KanbanTaskPatch`, then add:

```ts
export function updateKanbanTask(taskId: number, patch: KanbanTaskPatch): Promise<void> {
  return invoke<void>("update_kanban_task", { taskId, patch });
}

export function createKanbanLink(req: KanbanCreateLinkRequest): Promise<void> {
  return invoke<void>("create_kanban_link", { req });
}

export function removeKanbanLink(req: KanbanCreateLinkRequest): Promise<void> {
  return invoke<void>("remove_kanban_link", { req });
}
```

- [ ] **Step 3: Add Tauri commands**

In `crates/iota-desktop/src-tauri/src/lib.rs`, add after `transition_task`:

```rust
#[tauri::command]
async fn update_kanban_task(
    task_id: TaskId,
    patch: TaskPatch,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    {
        let store = state.kanban_store.lock().await;
        store.update_task(task_id, patch).map_err(|e| e.to_string())?;
    }
    let _ = tick_kanban_dispatcher(state.inner()).await;
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct DesktopKanbanLinkRequest {
    from_id: TaskId,
    to_id: TaskId,
    kind: LinkKind,
}

#[tauri::command]
async fn create_kanban_link(
    req: DesktopKanbanLinkRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let store = state.kanban_store.lock().await;
    store
        .create_link(req.from_id, req.to_id, req.kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_kanban_link(
    req: DesktopKanbanLinkRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let store = state.kanban_store.lock().await;
    store
        .remove_link(req.from_id, req.to_id, req.kind)
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Register commands**

Add these names to `tauri::generate_handler!`:

```rust
update_kanban_task,
create_kanban_link,
remove_kanban_link,
```

- [ ] **Step 5: Verify Rust and TypeScript compile**

Run:

```bash
cargo check
cd crates/iota-desktop && npm run build
```

Expected: both commands complete successfully.

## Task 3: Convert Kanban UI to Six-Column Board

**Files:**
- Modify: `crates/iota-desktop/src/components/KanbanWorkspace.tsx`

- [ ] **Step 1: Add board state**

Add state near existing state variables:

```ts
const [search, setSearch] = useState("");
const [selectedBoardId, setSelectedBoardId] = useState<number | "all">("all");
const [selectedAssignee, setSelectedAssignee] = useState<string | "all">("all");
const [lanesByProfile, setLanesByProfile] = useState(true);
```

- [ ] **Step 2: Derive filtered tasks and columns**

Replace `visibleTasks` with:

```ts
const filteredTasks = useMemo(
  () => filterKanbanTasks(tasks, { search, assignee: selectedAssignee, boardId: selectedBoardId }),
  [search, selectedAssignee, selectedBoardId, tasks],
);

const columns = useMemo(() => buildKanbanColumns(filteredTasks), [filteredTasks]);
const assignees = useMemo(() => uniqueAssignees(tasks), [tasks]);
```

- [ ] **Step 3: Replace status pills with controls**

In the header controls area, render:

```tsx
<input
  value={search}
  onChange={(event) => setSearch(event.target.value)}
  placeholder="Search tasks"
  className="h-8 min-w-0 rounded-lg border border-slate-800 bg-slate-950/30 px-3 text-xs text-slate-200 outline-none placeholder:text-slate-600"
/>
<select
  value={selectedBoardId}
  onChange={(event) => setSelectedBoardId(event.target.value === "all" ? "all" : Number(event.target.value))}
  className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
>
  <option value="all">All boards</option>
  {boards.map((board) => (
    <option key={board.id} value={board.id}>{board.slug}</option>
  ))}
</select>
<select
  value={selectedAssignee}
  onChange={(event) => setSelectedAssignee(event.target.value)}
  className="h-8 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-300 outline-none"
>
  <option value="all">All lanes</option>
  {assignees.map((assignee) => (
    <option key={assignee} value={assignee}>{assignee}</option>
  ))}
</select>
<label className="flex h-8 items-center gap-2 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-[11px] font-semibold text-slate-300">
  <input type="checkbox" checked={lanesByProfile} onChange={(event) => setLanesByProfile(event.target.checked)} />
  Lanes by profile
</label>
```

- [ ] **Step 4: Replace list rendering with six-column board**

Render columns in the main area:

```tsx
<div className="grid min-w-[980px] grid-cols-6 gap-3">
  {columns.map((column) => (
    <section key={column.status} className="min-h-[320px] rounded-lg border border-slate-800/80 bg-slate-950/20">
      <div className="flex items-center justify-between border-b border-slate-800/70 px-3 py-2">
        <span className="text-[11px] font-bold uppercase tracking-wide text-slate-300">{column.label}</span>
        <span className="rounded border border-slate-800 px-1.5 py-0.5 text-[10px] text-slate-500">{column.tasks.length}</span>
      </div>
      <div className="space-y-2 p-2">
        {column.status === "running" && lanesByProfile ? (
          groupRunningByLane(column.tasks).map((group) => (
            <div key={group.lane} className="space-y-2">
              <div className="px-1 text-[10px] font-bold uppercase tracking-wide text-slate-500">{group.lane} · {group.tasks.length}</div>
              {group.tasks.map((task) => renderTaskCard(task))}
            </div>
          ))
        ) : column.tasks.length === 0 ? (
          <div className="rounded-lg border border-dashed border-slate-800 p-3 text-center text-[11px] text-slate-600">Empty</div>
        ) : (
          column.tasks.map((task) => renderTaskCard(task))
        )}
      </div>
    </section>
  ))}
</div>
```

- [ ] **Step 5: Extract `renderTaskCard` local function**

Inside `KanbanWorkspace`, before `return`, define:

```tsx
const renderTaskCard = (task: KanbanTask) => (
  <article
    key={task.id}
    onClick={() => setSelectedTaskId(task.id)}
    className={`cursor-pointer rounded-lg border bg-slate-950/30 p-3 transition-colors ${
      selectedTaskId === task.id ? "border-primary/50 ring-1 ring-primary/25" : "border-slate-800/80 hover:border-slate-700"
    }`}
  >
    <div className="flex items-start justify-between gap-2">
      <div className="min-w-0">
        <div className="flex items-center gap-1.5 text-[10px] font-mono text-slate-500">
          <span>#{task.id}</span>
          <span>{formatBoardName(task, boards)}</span>
        </div>
        <h3 className="mt-1 line-clamp-2 text-xs font-bold leading-snug text-slate-100">{task.title}</h3>
      </div>
      <span className={`shrink-0 rounded-md border px-1.5 py-0.5 text-[9px] font-bold ${STATUS_STYLES[task.status]}`}>
        {STATUS_LABELS[task.status]}
      </span>
    </div>
    {task.body ? <p className="mt-2 line-clamp-3 text-[11px] leading-relaxed text-slate-400">{task.body}</p> : null}
    <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[10px] text-slate-500">
      <span className="rounded border border-slate-800 bg-slate-950/30 px-1.5 py-0.5">{task.assignee || "default"}</span>
      <span className="rounded border border-slate-800 bg-slate-950/30 px-1.5 py-0.5">P{task.priority}</span>
    </div>
  </article>
);
```

- [ ] **Step 6: Build frontend**

Run:

```bash
cd crates/iota-desktop && npm run build
```

Expected: TypeScript and Vite build successfully.

## Task 4: Make Task Detail a Drawer with Actions and Comments

**Files:**
- Modify: `crates/iota-desktop/src/components/KanbanWorkspace.tsx`

- [ ] **Step 1: Import new APIs and helper**

Update imports:

```ts
import { addKanbanComment, createKanbanLink, dispatchKanban, getKanbanTaskDetail, listenKanbanUpdates, listKanbanBoards, listKanbanTasks, transitionKanbanTask, updateKanbanTask } from "../api";
import { legalStatusActions } from "./kanbanModel";
```

If existing API names differ, define wrappers in `api.ts` with these exact names or adjust the import consistently.

- [ ] **Step 2: Add drawer form state**

Add:

```ts
const [commentBody, setCommentBody] = useState("");
const [taskActionPending, setTaskActionPending] = useState(false);
```

- [ ] **Step 3: Add action handlers**

Add:

```ts
const changeTaskStatus = async (toStatus: KanbanStatus) => {
  if (selectedTaskId === null) return;
  setTaskActionPending(true);
  try {
    await transitionKanbanTask(selectedTaskId, toStatus);
    await refresh();
  } catch (err) {
    setError(err instanceof Error ? err.message : String(err));
  } finally {
    setTaskActionPending(false);
  }
};

const submitComment = async () => {
  if (selectedTaskId === null || !commentBody.trim()) return;
  setTaskActionPending(true);
  try {
    await addKanbanComment(selectedTaskId, "desktop", commentBody.trim());
    setCommentBody("");
    await refresh();
  } catch (err) {
    setError(err instanceof Error ? err.message : String(err));
  } finally {
    setTaskActionPending(false);
  }
};
```

- [ ] **Step 4: Replace inline detail section with drawer**

Render the drawer as a right-side panel in the main flex layout:

```tsx
{selectedTaskId !== null ? (
  <aside className="flex w-[380px] shrink-0 flex-col border-l border-slate-800/80 bg-[#101422]">
    <div className="flex items-start justify-between gap-3 border-b border-slate-800/80 p-4">
      <div className="min-w-0">
        <div className="text-[10px] font-bold uppercase tracking-wider text-primary/90">Task Drawer</div>
        <h3 className="mt-1 truncate text-sm font-bold text-slate-100">{detail?.task.title ?? `#${selectedTaskId}`}</h3>
        <p className="mt-1 text-[11px] text-slate-500">{detailLoading ? "Loading detail..." : detail?.board?.slug ?? "iota kanban db"}</p>
      </div>
      <button type="button" onClick={() => setSelectedTaskId(null)} className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-slate-800 bg-slate-950/25 text-slate-400 hover:text-slate-100" aria-label="Close task drawer">
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      {/* Move existing detail body here, then add action buttons and comments form. */}
    </div>
  </aside>
) : null}
```

- [ ] **Step 5: Add legal action buttons**

Near the top of drawer body:

```tsx
{detail ? (
  <div className="mb-3 flex flex-wrap gap-2">
    {legalStatusActions(detail.task.status).map((action) => (
      <button
        key={action.to}
        type="button"
        onClick={() => changeTaskStatus(action.to)}
        disabled={taskActionPending}
        className="rounded-lg border border-primary/30 bg-primary/10 px-2.5 py-1.5 text-[11px] font-semibold text-primary hover:bg-primary/15 disabled:opacity-50"
      >
        {action.label}
      </button>
    ))}
  </div>
) : null}
```

- [ ] **Step 6: Add comments form**

Under comments list:

```tsx
<div className="mt-2 flex gap-2">
  <input
    value={commentBody}
    onChange={(event) => setCommentBody(event.target.value)}
    placeholder="Add comment"
    className="h-8 min-w-0 flex-1 rounded-lg border border-slate-800 bg-slate-950/30 px-2 text-xs text-slate-200 outline-none placeholder:text-slate-600"
  />
  <button
    type="button"
    onClick={submitComment}
    disabled={taskActionPending || !commentBody.trim()}
    className="h-8 rounded-lg bg-primary px-3 text-xs font-semibold text-white disabled:opacity-50"
  >
    Add
  </button>
</div>
```

- [ ] **Step 7: Build frontend**

Run:

```bash
cd crates/iota-desktop && npm run build
```

Expected: PASS.

## Task 5: New Task Form

**Files:**
- Modify: `crates/iota-desktop/src/components/KanbanWorkspace.tsx`
- Modify: `crates/iota-desktop/src/api.ts` if `createKanbanTask` wrapper is missing

- [ ] **Step 1: Add create task API wrapper if missing**

In `api.ts`:

```ts
export function createKanbanTask(req: {
  board_id: number;
  title: string;
  body?: string | null;
  status?: KanbanStatus;
  assignee?: string | null;
  priority?: number | null;
  tags: string[];
  workspace_kind?: string | null;
  workspace_path?: string | null;
}): Promise<number> {
  return invoke<number>("create_task", { req });
}
```

- [ ] **Step 2: Add form state**

In `KanbanWorkspace.tsx`:

```ts
const [creatingTask, setCreatingTask] = useState(false);
const [newTaskTitle, setNewTaskTitle] = useState("");
const [newTaskBody, setNewTaskBody] = useState("");
const [newTaskAssignee, setNewTaskAssignee] = useState("");
const [newTaskStatus, setNewTaskStatus] = useState<KanbanStatus>("ready");
```

- [ ] **Step 3: Add submit handler**

```ts
const submitNewTask = async () => {
  const boardId = selectedBoardId === "all" ? boards[0]?.id : selectedBoardId;
  if (!boardId || !newTaskTitle.trim()) return;
  setTaskActionPending(true);
  try {
    const taskId = await createKanbanTask({
      board_id: boardId,
      title: newTaskTitle.trim(),
      body: newTaskBody.trim() || null,
      status: newTaskStatus,
      assignee: newTaskAssignee.trim() || null,
      priority: 0,
      tags: [],
      workspace_kind: null,
      workspace_path: null,
    });
    setCreatingTask(false);
    setNewTaskTitle("");
    setNewTaskBody("");
    setNewTaskAssignee("");
    setSelectedTaskId(taskId);
    await refresh({ tick: newTaskStatus === "ready" });
  } catch (err) {
    setError(err instanceof Error ? err.message : String(err));
  } finally {
    setTaskActionPending(false);
  }
};
```

- [ ] **Step 4: Add `New task` button and compact form**

Add a header button and render the form above board columns when `creatingTask` is true. The form must include title, body, assignee, status, create, and cancel.

- [ ] **Step 5: Build frontend**

Run:

```bash
cd crates/iota-desktop && npm run build
```

Expected: PASS.

## Task 6: Full Verification

**Files:**
- No code files unless fixing issues found by verification.

- [ ] **Step 1: Run frontend tests**

Run:

```bash
cd crates/iota-desktop && npm test
```

Expected: all Node tests pass.

- [ ] **Step 2: Run frontend build**

Run:

```bash
cd crates/iota-desktop && npm run build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 3: Run Rust check**

Run:

```bash
cargo check
```

Expected: workspace check passes.

- [ ] **Step 4: Start desktop dev server**

Run:

```bash
cd crates/iota-desktop && npm run dev:clean
```

Expected: desktop app starts. If it blocks as a long-running process, keep it running only long enough to verify startup logs and then stop it before finishing.

- [ ] **Step 5: Manual behavior checklist**

Verify:

- Six visible board columns exist.
- `Nudge dispatcher` updates last tick text.
- Running tasks group by lane when toggle is on.
- Drawer opens on card click and includes run history, events, comments, and logs.
- Adding a comment refreshes drawer comments.
- Blocked task shows `Unblock`.
- Creating a ready task can trigger a dispatcher tick.

