use anyhow::Result;

use crate::kanban::{Board, KanbanStore, Link, Run, Status, Task, TaskFilter, TaskId};

const STATUS_COLUMNS: [Status; 6] = [
    Status::Triage,
    Status::Todo,
    Status::Ready,
    Status::Running,
    Status::Blocked,
    Status::Done,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KanbanViewMode {
    Columns,
    List,
    Graph,
    Timeline,
}

impl KanbanViewMode {
    pub(super) fn next(self) -> Self {
        match self {
            Self::Columns => Self::List,
            Self::List => Self::Graph,
            Self::Graph => Self::Timeline,
            Self::Timeline => Self::Columns,
        }
    }

    pub(super) fn from_digit(ch: char) -> Option<Self> {
        match ch {
            '1' => Some(Self::Columns),
            '2' => Some(Self::List),
            '3' => Some(Self::Graph),
            '4' => Some(Self::Timeline),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::List => "list",
            Self::Graph => "graph",
            Self::Timeline => "timeline",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct KanbanViewState {
    pub active: bool,
    pub mode: KanbanViewMode,
    pub board_slug: Option<String>,
    pub filter: String,
    pub selected_column: usize,
    pub selected_task_id: Option<TaskId>,
    pub detail_open: bool,
}

impl Default for KanbanViewState {
    fn default() -> Self {
        Self {
            active: false,
            mode: KanbanViewMode::Columns,
            board_slug: None,
            filter: String::new(),
            selected_column: 0,
            selected_task_id: None,
            detail_open: true,
        }
    }
}

impl KanbanViewState {
    pub(super) fn open(&mut self, board_slug: Option<String>) {
        self.active = true;
        if board_slug.is_some() {
            self.board_slug = board_slug;
            self.selected_task_id = None;
        }
    }

    pub(super) fn close(&mut self) {
        self.active = false;
    }

    pub(super) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
    }

    pub(super) fn set_filter(&mut self, filter: String) {
        self.filter = filter.trim().to_string();
        self.selected_task_id = None;
    }

    pub(super) fn select_column_delta(&mut self, delta: isize, snapshot: &KanbanSnapshot) {
        let current = self.selected_column as isize;
        let max = STATUS_COLUMNS.len() as isize - 1;
        self.selected_column = (current + delta).clamp(0, max) as usize;
        self.selected_task_id = tasks_for_column(self, snapshot, self.selected_column)
            .first()
            .map(|task| task.task.id);
    }

    pub(super) fn select_task_delta(&mut self, delta: isize, snapshot: &KanbanSnapshot) {
        let tasks = selectable_tasks(self, snapshot);
        if tasks.is_empty() {
            self.selected_task_id = None;
            return;
        }
        let current = self
            .selected_task_id
            .and_then(|id| tasks.iter().position(|task| task.task.id == id))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, tasks.len() as isize - 1) as usize;
        self.selected_task_id = Some(tasks[next].task.id);
    }

    pub(super) fn selected_task_id(&mut self, snapshot: &KanbanSnapshot) -> Option<TaskId> {
        if let Some(id) = self.selected_task_id
            && selectable_tasks(self, snapshot)
                .iter()
                .any(|task| task.task.id == id)
        {
            return Some(id);
        }
        self.selected_task_id = selectable_tasks(self, snapshot)
            .first()
            .map(|task| task.task.id);
        self.selected_task_id
    }
}

#[derive(Debug, Clone)]
pub(super) struct KanbanTaskSnapshot {
    pub task: Task,
    pub links: Vec<Link>,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone)]
pub(super) struct KanbanSnapshot {
    pub board: Option<Board>,
    pub tasks: Vec<KanbanTaskSnapshot>,
}

impl KanbanSnapshot {
    pub(super) fn load(store: &dyn KanbanStore, board_slug: Option<&str>) -> Result<Self> {
        let board = if let Some(slug) = board_slug {
            Some(store.get_board(slug)?)
        } else {
            store.list_boards()?.into_iter().next()
        };

        let tasks = if let Some(board) = &board {
            store.list_tasks(TaskFilter {
                board_id: Some(board.id),
                limit: Some(300),
                ..Default::default()
            })?
        } else {
            Vec::new()
        };

        let mut snapshots = Vec::with_capacity(tasks.len());
        for task in tasks {
            let links = store.get_links(task.id).unwrap_or_default();
            let runs = store.get_runs(task.id).unwrap_or_default();
            snapshots.push(KanbanTaskSnapshot { task, links, runs });
        }
        snapshots.sort_by_key(|item| {
            (
                status_index(item.task.status),
                -item.task.priority,
                item.task.id,
            )
        });

        Ok(Self {
            board,
            tasks: snapshots,
        })
    }
}

pub(super) fn render_lines(
    state: &mut KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    height: u16,
) -> Vec<String> {
    let _ = state.selected_task_id(snapshot);
    let mut lines = Vec::new();
    let board = snapshot
        .board
        .as_ref()
        .map(|board| format!("{} ({})", board.name, board.slug))
        .unwrap_or_else(|| "No board".to_string());
    lines.push(fit_line(
        format!(
            "Kanban: {} | mode {} | filter {} | 1 columns 2 list 3 graph 4 timeline | j/k tab enter",
            board,
            state.mode.label(),
            if state.filter.is_empty() {
                "-"
            } else {
                state.filter.as_str()
            }
        ),
        width,
    ));

    match state.mode {
        KanbanViewMode::Columns => render_columns(state, snapshot, width, &mut lines),
        KanbanViewMode::List => render_list(state, snapshot, width, &mut lines),
        KanbanViewMode::Graph => render_graph(state, snapshot, width, &mut lines),
        KanbanViewMode::Timeline => render_timeline(state, snapshot, width, &mut lines),
    }

    if state.detail_open {
        render_detail(state, snapshot, width, &mut lines);
    }

    while lines.len() < height as usize {
        lines.push(String::new());
    }
    lines.truncate(height as usize);
    lines
}

fn render_columns(
    state: &KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    lines: &mut Vec<String>,
) {
    let header = STATUS_COLUMNS
        .iter()
        .enumerate()
        .map(|(idx, status)| {
            let count = tasks_for_column(state, snapshot, idx).len();
            if idx == state.selected_column {
                format!("[{}:{}]", status.as_str(), count)
            } else {
                format!(" {}:{} ", status.as_str(), count)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    lines.push(fit_line(header, width));

    for idx in 0..STATUS_COLUMNS.len() {
        let tasks = tasks_for_column(state, snapshot, idx);
        if tasks.is_empty() {
            continue;
        }
        let prefix = if idx == state.selected_column {
            ">"
        } else {
            " "
        };
        let summary = tasks
            .iter()
            .take(4)
            .map(|item| task_label(state, item))
            .collect::<Vec<_>>()
            .join("  ");
        lines.push(fit_line(
            format!("{} {:<8} {}", prefix, STATUS_COLUMNS[idx].as_str(), summary),
            width,
        ));
    }
}

fn render_list(
    state: &KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    lines: &mut Vec<String>,
) {
    for item in selectable_tasks(state, snapshot).iter().take(12) {
        lines.push(fit_line(
            format!(
                "{} #{} [{:<8}] p{} {}",
                selected_marker(state, item),
                item.task.id,
                item.task.status.as_str(),
                item.task.priority,
                item.task.title
            ),
            width,
        ));
    }
}

fn render_graph(
    state: &KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    lines: &mut Vec<String>,
) {
    for item in selectable_tasks(state, snapshot).iter().take(12) {
        let links = item
            .links
            .iter()
            .map(|link| {
                if link.from_id == item.task.id {
                    format!("{}->#{}", link.kind, link.to_id)
                } else {
                    format!("#{}->{}", link.from_id, link.kind)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(fit_line(
            format!(
                "{} #{} {}{}",
                selected_marker(state, item),
                item.task.id,
                item.task.title,
                if links.is_empty() {
                    String::new()
                } else {
                    format!(" | {}", links)
                }
            ),
            width,
        ));
    }
}

fn render_timeline(
    state: &KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    lines: &mut Vec<String>,
) {
    let mut rows: Vec<(&KanbanTaskSnapshot, &Run)> = snapshot
        .tasks
        .iter()
        .filter(|task| matches_filter(task, &state.filter))
        .flat_map(|task| task.runs.iter().map(move |run| (task, run)))
        .collect();
    rows.sort_by_key(|(_, run)| -run.started_at);
    if rows.is_empty() {
        lines.push("No worker runs yet.".to_string());
        return;
    }
    for (task, run) in rows.into_iter().take(12) {
        lines.push(fit_line(
            format!(
                "{} #{} {} | {} | {}",
                selected_marker(state, task),
                task.task.id,
                task.task.title,
                run.profile,
                run.status
            ),
            width,
        ));
    }
}

fn render_detail(
    state: &KanbanViewState,
    snapshot: &KanbanSnapshot,
    width: u16,
    lines: &mut Vec<String>,
) {
    let Some(id) = state.selected_task_id else {
        return;
    };
    let Some(item) = snapshot.tasks.iter().find(|item| item.task.id == id) else {
        return;
    };
    lines.push(fit_line(
        format!("-- #{} {}", item.task.id, item.task.title),
        width,
    ));
    lines.push(fit_line(
        format!(
            "status {} | priority {} | assignee {} | tags {}",
            item.task.status,
            item.task.priority,
            item.task.assignee.as_deref().unwrap_or("-"),
            if item.task.tags.is_empty() {
                "-".to_string()
            } else {
                item.task.tags.join(",")
            }
        ),
        width,
    ));
    if let Some(body) = &item.task.body {
        lines.push(fit_line(body.replace('\n', " "), width));
    }
}

fn selectable_tasks<'a>(
    state: &KanbanViewState,
    snapshot: &'a KanbanSnapshot,
) -> Vec<&'a KanbanTaskSnapshot> {
    if state.mode == KanbanViewMode::Columns {
        return tasks_for_column(state, snapshot, state.selected_column);
    }
    snapshot
        .tasks
        .iter()
        .filter(|item| matches_filter(item, &state.filter))
        .collect()
}

fn tasks_for_column<'a>(
    state: &KanbanViewState,
    snapshot: &'a KanbanSnapshot,
    column: usize,
) -> Vec<&'a KanbanTaskSnapshot> {
    let status = STATUS_COLUMNS
        .get(column)
        .copied()
        .unwrap_or(Status::Triage);
    snapshot
        .tasks
        .iter()
        .filter(|item| item.task.status == status && matches_filter(item, &state.filter))
        .collect()
}

fn matches_filter(item: &KanbanTaskSnapshot, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let filter = filter.to_ascii_lowercase();
    item.task.title.to_ascii_lowercase().contains(&filter)
        || item
            .task
            .body
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains(&filter)
        || item
            .task
            .tags
            .iter()
            .any(|tag| tag.to_ascii_lowercase().contains(&filter))
}

fn task_label(state: &KanbanViewState, item: &KanbanTaskSnapshot) -> String {
    format!(
        "{}#{} {}",
        selected_marker(state, item),
        item.task.id,
        item.task.title
    )
}

fn selected_marker(state: &KanbanViewState, item: &KanbanTaskSnapshot) -> &'static str {
    if state.selected_task_id == Some(item.task.id) {
        "*"
    } else {
        " "
    }
}

fn status_index(status: Status) -> usize {
    STATUS_COLUMNS
        .iter()
        .position(|item| *item == status)
        .unwrap_or(STATUS_COLUMNS.len())
}

fn fit_line(line: String, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }
    let mut out: String = line.chars().take(width).collect();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::kanban::{LinkKind, RunStatus};

    use super::*;

    fn task(id: TaskId, status: Status, title: &str) -> KanbanTaskSnapshot {
        KanbanTaskSnapshot {
            task: Task {
                id,
                board_id: 1,
                title: title.to_string(),
                body: Some(format!("body {id}")),
                status,
                assignee: None,
                priority: id as i32,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
                created_at: id as i64,
                updated_at: id as i64,
                claimed_at: None,
                claim_ttl_secs: 1800,
            },
            links: vec![],
            runs: vec![],
        }
    }

    fn snapshot() -> KanbanSnapshot {
        KanbanSnapshot {
            board: Some(Board {
                id: 1,
                slug: "dev".to_string(),
                name: "Dev".to_string(),
                created_at: 1,
            }),
            tasks: vec![
                task(1, Status::Todo, "write tests"),
                task(2, Status::Ready, "wire tui"),
                task(3, Status::Done, "review spec"),
            ],
        }
    }

    #[test]
    fn column_view_groups_tasks_and_selects_first_task() {
        let mut state = KanbanViewState::default();
        state.open(Some("dev".to_string()));
        state.selected_column = 1;

        let lines = render_lines(&mut state, &snapshot(), 100, 8).join("\n");

        assert_eq!(state.selected_task_id, Some(1));
        assert!(lines.contains("[todo:1]"));
        assert!(lines.contains("*#1 write tests"));
    }

    #[test]
    fn list_view_selects_and_moves_through_tasks() {
        let snapshot = snapshot();
        let mut state = KanbanViewState {
            active: true,
            mode: KanbanViewMode::List,
            ..Default::default()
        };

        state.select_task_delta(1, &snapshot);

        assert_eq!(state.selected_task_id, Some(2));
    }

    #[test]
    fn graph_view_renders_task_links() {
        let mut snapshot = snapshot();
        snapshot.tasks[0].links.push(Link {
            from_id: 1,
            to_id: 2,
            kind: LinkKind::Blocks,
        });
        let mut state = KanbanViewState {
            active: true,
            mode: KanbanViewMode::Graph,
            ..Default::default()
        };

        let lines = render_lines(&mut state, &snapshot, 100, 8).join("\n");

        assert!(lines.contains("blocks->#2"));
    }

    #[test]
    fn timeline_view_renders_worker_runs() {
        let mut snapshot = snapshot();
        snapshot.tasks[1].runs.push(Run {
            id: "run-1".to_string(),
            task_id: 2,
            profile: "hermes".to_string(),
            status: RunStatus::Completed,
            started_at: 10,
            finished_at: Some(20),
            last_heartbeat: 20,
            exit_code: Some(0),
            output_summary: None,
        });
        let mut state = KanbanViewState {
            active: true,
            mode: KanbanViewMode::Timeline,
            ..Default::default()
        };

        let lines = render_lines(&mut state, &snapshot, 100, 8).join("\n");

        assert!(lines.contains("hermes"));
        assert!(lines.contains("completed"));
    }
}
