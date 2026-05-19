//! Kanban live demo — create a task from a prompt and watch it run in real-time.
//!
//! Usage:
//!   cargo run --example kanban_live_demo -- "Build a login module"
//!   cargo run --example kanban_live_demo          # defaults to a generic prompt

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Duration, sleep};

use iota_sympantos::kanban::{
    CreateTaskRequest, KanbanStore, KanbanUiEvent, RunStatus, SqliteKanbanStore, Status,
    TaskFilter,
};

#[tokio::main]
async fn main() -> Result<()> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Implement a demo feature".to_string());

    println!("+-----------------------------------------+");
    println!("|  iota-sympantos . Kanban Live Demo      |");
    println!("+-----------------------------------------+");
    println!();
    println!("Prompt: {}", prompt);
    println!();

    // Store (in-memory for demo; swap ":memory:" for a file path to persist)
    let concrete = Arc::new(SqliteKanbanStore::open(Path::new(":memory:"))?);
    let mut rx = concrete.subscribe();           // subscribe before coercion
    let store: Arc<dyn KanbanStore> = concrete;

    // 1. Create board + task from prompt
    let board_id = store.create_board("demo", "Demo Board")?;
    println!("[init] board created: demo");

    let task_id = store.create_task(CreateTaskRequest {
        board_id,
        title: prompt.clone(),
        body: Some(format!("Auto-created from prompt:\n> {}", prompt)),
        status: Some(Status::Triage),
        assignee: Some("demo-worker".to_string()),
        priority: Some(5),
        tags: vec!["auto".to_string()],
        workspace_kind: None,
        workspace_path: None,
    })?;
    println!("[init] task  created: #{} -- {}", task_id, prompt);
    println!();
    print_board(&store, board_id)?;

    // 2. Background worker -- simulate full task lifecycle:
    //    Triage -> Todo -> Ready -> Running (with heartbeats) -> Done
    //
    //    In production this work is done by a real hermes worker process.
    //    Here we drive the same KanbanStore API so the demo runs standalone.
    let bg = store.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(800)).await;
        let _ = bg.transition(task_id, Status::Todo);

        sleep(Duration::from_millis(800)).await;
        let _ = bg.transition(task_id, Status::Ready);
        let _ = bg.add_comment(task_id, "scheduler", "Task queued for execution.");

        sleep(Duration::from_millis(600)).await;
        let _ = bg.transition(task_id, Status::Running);
        let run_id = match bg.create_run(task_id, "demo-worker") {
            Ok(id) => id,
            Err(_) => return,
        };

        for i in 1u32..=3 {
            sleep(Duration::from_secs(2)).await;
            let _ = bg.heartbeat(&run_id);
            let _ = bg.add_comment(task_id, "worker", &format!("Step {}/3 complete.", i));
        }

        let _ = bg.complete_run(&run_id, RunStatus::Completed, Some(0));
        let _ = bg.add_comment(task_id, "system", "All steps finished successfully.");
        let _ = bg.transition(task_id, Status::Done);
    });

    // 3. Event loop -- print the board on each status change, exit when Done
    loop {
        match rx.recv().await {
            Ok(event) => {
                if handle_event(&event, &store, board_id)? {
                    break;
                }
            }
            Err(RecvError::Lagged(n)) => eprintln!("[warn] {} events dropped", n),
            Err(RecvError::Closed) => break,
        }
    }

    // 4. Final summary
    final_summary(&store, task_id, board_id)
}

fn final_summary(store: &Arc<dyn KanbanStore>, task_id: u64, board_id: u64) -> Result<()> {
    println!();
    println!("==========================================");
    println!(" Completed -- final state");
    println!("==========================================");
    print_board(store, board_id)?;

    let task     = store.get_task(task_id)?;
    let comments = store.list_comments(task_id)?;
    let runs     = store.get_runs(task_id)?;

    println!("Task #{}", task.id);
    println!("  status   : {}", task.status);
    println!("  assignee : {}", task.assignee.as_deref().unwrap_or("-"));
    println!("  priority : {}", task.priority);
    println!();
    println!("Comments ({}):", comments.len());
    for c in &comments {
        println!("  [{}] {}", c.author, c.body);
    }
    println!();
    println!("Runs ({}):", runs.len());
    for r in &runs {
        println!(
            "  {} -> {} (exit={:?})",
            &r.id[..r.id.len().min(8)],
            r.status,
            r.exit_code
        );
    }
    Ok(())
}

// Returns true when the event signals a terminal status (Done or Archived).
fn handle_event(
    event: &KanbanUiEvent,
    store: &Arc<dyn KanbanStore>,
    board_id: u64,
) -> Result<bool> {
    match event {
        KanbanUiEvent::TaskStatusChanged { id, from, to } => {
            println!("-- #{} : {} -> {}", id, from, to);
            print_board(store, board_id)?;
            return Ok(matches!(to, Status::Done | Status::Archived));
        }
        KanbanUiEvent::RunStarted { task_id, run_id } => {
            println!(
                "   > run started  task #{} / {}",
                task_id,
                &run_id[..run_id.len().min(8)]
            );
        }
        KanbanUiEvent::RunCompleted { task_id, run_id, status } => {
            println!(
                "   * run finished task #{} / {} -> {}",
                task_id,
                &run_id[..run_id.len().min(8)],
                status
            );
        }
        KanbanUiEvent::CommentAdded { task_id, .. } => {
            if let Ok(cs) = store.list_comments(*task_id) {
                if let Some(c) = cs.last() {
                    println!("   ~ [{}] {}", c.author, c.body);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ASCII board renderer — one column per status, tasks listed vertically.
fn print_board(store: &Arc<dyn KanbanStore>, board_id: u64) -> Result<()> {
    let boards = store.list_boards()?;
    let Some(board) = boards.iter().find(|b| b.id == board_id) else {
        return Ok(());
    };
    let tasks = store.list_tasks(TaskFilter {
        board_id: Some(board_id),
        limit: Some(100),
        ..Default::default()
    })?;

    let cols: &[Status] = &[
        Status::Triage,
        Status::Todo,
        Status::Ready,
        Status::Running,
        Status::Blocked,
        Status::Done,
    ];
    let cw: usize = 13; // content width per cell (chars)
    let total = (cw + 3) * cols.len() + 1;

    // Top border + board name
    let title = format!(" {} ", board.name);
    let fill = total.saturating_sub(title.len() + 2);
    println!("+{}+{:-<fill$}+", title, "", fill = fill);

    // Column headers with task counts
    let mut header = String::from("|");
    for col in cols {
        let n = tasks.iter().filter(|t| t.status == *col).count();
        let lbl = format!("{}({})", col.as_str().to_uppercase(), n);
        header.push_str(&format!(" {:<width$} |", lbl, width = cw));
    }
    println!("{}", header);

    // Separator row
    let mut sep = String::from("|");
    for _ in cols {
        sep.push_str(&format!(" {:-<width$} |", "", width = cw));
    }
    println!("{}", sep);

    // Task rows (up to 6 visible rows per column)
    let grouped: Vec<Vec<_>> = cols
        .iter()
        .map(|c| tasks.iter().filter(|t| t.status == *c).collect())
        .collect();
    let rows = grouped.iter().map(|g| g.len()).max().unwrap_or(0).min(6).max(1);

    for row in 0..rows {
        let mut line = String::from("|");
        for (i, col_tasks) in grouped.iter().enumerate() {
            let cell = if row < col_tasks.len() {
                let t = col_tasks[row];
                let mark = if cols[i] == Status::Running { ">" } else { " " };
                let short: String = t.title.chars().take(cw.saturating_sub(4)).collect();
                format!("{} #{} {}", mark, t.id, short)
            } else {
                String::new()
            };
            line.push_str(&format!(" {:<width$} |", cell, width = cw));
        }
        println!("{}", line);
    }

    println!("+{:-<width$}+", "", width = total - 2);
    println!();
    Ok(())
}
