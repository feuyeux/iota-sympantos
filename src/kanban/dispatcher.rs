use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use super::shadow::{ShadowMaterializer, ShadowWatcher};
use super::store::KanbanStore;
use super::types::*;
use super::worker::{WorkerConfig, WorkerEnv, WorkerHandle, build_worker_context};

// ---------------------------------------------------------------------------
// DispatcherConfig
// ---------------------------------------------------------------------------

pub struct DispatcherConfig {
    /// How often tick() is expected to be called (informational).
    pub tick_interval: Duration,
    /// Maximum number of concurrent workers.
    pub max_concurrent: usize,
    /// Kill a worker that has been running for this long regardless of heartbeats.
    pub claim_ttl: Duration,
    /// Kill a worker whose run has not sent a heartbeat within this window.
    pub heartbeat_timeout: Duration,
    /// Path to the hermes binary.
    pub hermes_bin: PathBuf,
    /// Directory where shadow databases are materialised.
    pub shadows_dir: PathBuf,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_secs(30),
            max_concurrent: 4,
            claim_ttl: Duration::from_secs(900), // 15 min
            heartbeat_timeout: Duration::from_secs(90),
            hermes_bin: PathBuf::from("hermes"),
            shadows_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".i6/kanban/shadows"),
        }
    }
}

// ---------------------------------------------------------------------------
// TickReport
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct TickReport {
    pub spawned: usize,
    pub completed: usize,
    pub timed_out: usize,
    pub spawn_failures: usize,
    pub reclaimed: usize,
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

pub struct Dispatcher {
    config: DispatcherConfig,
    workers: HashMap<TaskId, (WorkerHandle, ShadowWatcher)>,
    materializer: ShadowMaterializer,
    worker_config: WorkerConfig,
}

impl Dispatcher {
    pub fn new(config: DispatcherConfig) -> Self {
        let materializer = ShadowMaterializer::new(config.shadows_dir.clone());
        let worker_config = WorkerConfig {
            hermes_bin: config.hermes_bin.clone(),
        };
        Self {
            config,
            workers: HashMap::new(),
            materializer,
            worker_config,
        }
    }

    pub fn tick_interval(&self) -> Duration {
        self.config.tick_interval
    }

    #[cfg(test)]
    pub fn set_hermes_bin_for_tests(&mut self, hermes_bin: PathBuf) {
        self.config.hermes_bin = hermes_bin.clone();
        self.worker_config.hermes_bin = hermes_bin;
    }

    /// Number of workers currently active.
    pub fn active_worker_count(&self) -> usize {
        self.workers.len()
    }

    // -----------------------------------------------------------------------
    // tick
    // -----------------------------------------------------------------------

    /// One iteration of the dispatcher loop.
    pub fn tick(&mut self, store: &dyn KanbanStore) -> Result<TickReport> {
        let mut report = self.health_check(store)?;

        report.reclaimed += self.reclaim_expired_running(store)?;

        self.recompute_ready(store)?;

        // Spawn new workers for Ready tasks up to the concurrency limit
        let available = self
            .config
            .max_concurrent
            .saturating_sub(self.workers.len());
        if available > 0 {
            let ready_tasks = store.list_tasks(TaskFilter {
                status: Some(Status::Ready),
                limit: Some(available),
                ..Default::default()
            })?;

            for task in ready_tasks {
                if self.workers.len() >= self.config.max_concurrent {
                    break;
                }
                if self.workers.contains_key(&task.id) {
                    continue;
                }
                match self.spawn_worker(&task, store) {
                    Ok(()) => report.spawned += 1,
                    Err(e) => {
                        tracing::warn!(
                            task_id = task.id,
                            error = %e,
                            "failed to spawn worker"
                        );
                        report.spawn_failures += 1;
                    }
                }
            }
        }

        Ok(report)
    }

    // -----------------------------------------------------------------------
    // health_check
    // -----------------------------------------------------------------------

    fn health_check(&mut self, store: &dyn KanbanStore) -> Result<TickReport> {
        let mut report = TickReport::default();
        let task_ids: Vec<TaskId> = self.workers.keys().copied().collect();
        let mut to_remove: Vec<TaskId> = Vec::new();

        for task_id in task_ids {
            // Collect handle info -- drop the borrow before calling store methods
            let (run_id, elapsed, exit_code) = {
                let entry = self.workers.get_mut(&task_id).unwrap();
                (
                    entry.0.run_id.clone(),
                    entry.0.elapsed_secs(),
                    entry.0.is_finished(),
                )
            };

            // Poll watcher for new shadow events
            let (events, terminal_status) = {
                let entry = self.workers.get_mut(&task_id).unwrap();
                entry.1.poll().unwrap_or_default()
            };

            // Sync events into the store
            if !events.is_empty() {
                let entry = self.workers.get_mut(&task_id).unwrap();
                match entry.1.sync_events(&events, store, &run_id) {
                    Ok(()) => entry.1.mark_events_synced(&events),
                    Err(e) => {
                        tracing::warn!(
                            task_id,
                            run_id = %run_id,
                            error = %e,
                            "failed to sync shadow events"
                        );
                    }
                }
            }

            // --- Claim TTL exceeded ---
            if elapsed > self.config.claim_ttl.as_secs() {
                {
                    let entry = self.workers.get_mut(&task_id).unwrap();
                    let _ = entry.0.kill();
                }
                let _ = store.complete_run(&run_id, RunStatus::TimedOut, None);
                let _ = store.transition(task_id, Status::Ready);
                let _ = self.materializer.cleanup(task_id);
                report.timed_out += 1;
                to_remove.push(task_id);
                continue;
            }

            // --- Heartbeat timeout ---
            let heartbeat_expired = store
                .get_runs(task_id)
                .ok()
                .and_then(|runs| runs.into_iter().find(|r| r.id == run_id))
                .map(|run| {
                    let now = crate::utils::now_ts();
                    (now - run.last_heartbeat) > self.config.heartbeat_timeout.as_secs() as i64
                })
                .unwrap_or(false);

            if heartbeat_expired {
                {
                    let entry = self.workers.get_mut(&task_id).unwrap();
                    let _ = entry.0.kill();
                }
                let _ = store.complete_run(&run_id, RunStatus::TimedOut, None);
                let _ = store.transition(task_id, Status::Ready);
                let _ = self.materializer.cleanup(task_id);
                report.timed_out += 1;
                to_remove.push(task_id);
                continue;
            }

            // --- Terminal status from shadow or process has exited ---
            if terminal_status.is_some() || exit_code.is_some() {
                if terminal_status.is_some() && exit_code.is_none() {
                    let entry = self.workers.get_mut(&task_id).unwrap();
                    let _ = entry.0.kill();
                }
                let failed = exit_code.map(|c| c != 0).unwrap_or(false);
                let rs = if failed {
                    RunStatus::Failed
                } else {
                    RunStatus::Completed
                };
                let _ = store.complete_run(&run_id, rs, exit_code);
                if failed {
                    // Non-zero exit: transition task back to Ready for retry
                    let _ = store.transition(task_id, Status::Ready);
                } else if let Some(status) = terminal_status.as_deref() {
                    match status {
                        "done" => {
                            let _ = store.transition(task_id, Status::Done);
                        }
                        "blocked" => {
                            let _ = store.transition(task_id, Status::Blocked);
                        }
                        _ => {}
                    }
                } else {
                    let _ = store.transition(task_id, Status::Done);
                }
                let _ = self.materializer.cleanup(task_id);
                report.completed += 1;
                to_remove.push(task_id);
            }
        }

        for task_id in to_remove {
            self.workers.remove(&task_id);
        }

        Ok(report)
    }

    // -----------------------------------------------------------------------
    // recompute_ready
    // -----------------------------------------------------------------------

    fn recompute_ready(&mut self, store: &dyn KanbanStore) -> Result<()> {
        let blocked = store.list_tasks(TaskFilter {
            status: Some(Status::Blocked),
            ..Default::default()
        })?;

        for task in blocked {
            let links = store.get_links(task.id)?;

            // Collect all task IDs that are blocking this one
            let blockers: Vec<TaskId> = links
                .iter()
                .filter(|l| l.to_id == task.id && l.kind == LinkKind::Blocks)
                .map(|l| l.from_id)
                .collect();

            // If there are no blockers, nothing to check
            if blockers.is_empty() {
                continue;
            }

            // Unblock when every blocker has reached a terminal status
            let all_done = blockers.iter().all(|&blocker_id| {
                store
                    .get_task(blocker_id)
                    .map(|t| t.status == Status::Done || t.status == Status::Archived)
                    .unwrap_or(false)
            });

            if all_done {
                let _ = store.transition(task.id, Status::Ready);
            }
        }

        Ok(())
    }

    fn reclaim_expired_running(&mut self, store: &dyn KanbanStore) -> Result<usize> {
        let running = store.list_tasks(TaskFilter {
            status: Some(Status::Running),
            ..Default::default()
        })?;
        let now = crate::utils::now_ts();
        let mut reclaimed = 0;

        for task in running {
            if self.workers.contains_key(&task.id) {
                continue;
            }
            let Some(claimed_at) = task.claimed_at else {
                continue;
            };
            let ttl = std::cmp::min(
                task.claim_ttl_secs.max(0),
                self.config.claim_ttl.as_secs() as i64,
            );
            if now - claimed_at >= ttl {
                for run in store
                    .get_runs(task.id)?
                    .into_iter()
                    .filter(|run| run.status == RunStatus::Running)
                {
                    store.complete_run(&run.id, RunStatus::TimedOut, None)?;
                }
                store.transition(task.id, Status::Ready)?;
                reclaimed += 1;
            }
        }

        Ok(reclaimed)
    }

    // -----------------------------------------------------------------------
    // spawn_worker
    // -----------------------------------------------------------------------

    fn spawn_worker(&mut self, task: &Task, store: &dyn KanbanStore) -> Result<()> {
        let profile = task.assignee.as_deref().unwrap_or("default").to_string();

        // Transition to Running
        store.transition(task.id, Status::Running)?;

        // Create a run record
        let run_id = match store.create_run(task.id, &profile) {
            Ok(run_id) => run_id,
            Err(e) => {
                let _ = store.transition(task.id, Status::Ready);
                return Err(e);
            }
        };

        let spawn_result = (|| -> Result<(WorkerHandle, ShadowWatcher)> {
            // Locate the board
            let boards = store.list_boards()?;
            let board = boards
                .into_iter()
                .find(|b| b.id == task.board_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("board {} not found for task {}", task.board_id, task.id)
                })?;

            // Materialise the shadow DB
            let shadow_db = self.materializer.materialize(task, &board, store)?;

            // Build the markdown context
            let comments = store.list_comments(task.id)?;
            let prior_runs: Vec<Run> = store
                .get_runs(task.id)?
                .into_iter()
                .filter(|r| r.id != run_id)
                .collect();
            let context = build_worker_context(task, &comments, &prior_runs);

            // Spawn the worker process
            let env = WorkerEnv {
                task_id: task.id,
                run_id: run_id.clone(),
                shadow_path: shadow_db.path.clone(),
                board_slug: board.slug,
                profile,
            };
            let handle = WorkerHandle::spawn(&self.worker_config, env, &context)?;

            // Create a watcher for the shadow DB
            let watcher = ShadowWatcher::new(shadow_db.path, task.id);
            Ok((handle, watcher))
        })();

        let (handle, watcher) = match spawn_result {
            Ok(result) => result,
            Err(e) => {
                let _ = store.complete_run(&run_id, RunStatus::Failed, None);
                let _ = store.transition(task.id, Status::Ready);
                let _ = self.materializer.cleanup(task.id);
                return Err(e);
            }
        };

        self.workers.insert(task.id, (handle, watcher));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::sqlite_store::SqliteKanbanStore;
    use super::super::store::KanbanStore;
    use super::*;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iota-disp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dispatcher_config_defaults() {
        let cfg = DispatcherConfig::default();
        assert_eq!(cfg.tick_interval, Duration::from_secs(30));
        assert_eq!(cfg.max_concurrent, 4);
        assert_eq!(cfg.claim_ttl, Duration::from_secs(900));
        assert_eq!(cfg.heartbeat_timeout, Duration::from_secs(90));
        assert_eq!(cfg.hermes_bin, PathBuf::from("hermes"));
    }

    #[test]
    fn dispatcher_new_has_no_workers() {
        let cfg = DispatcherConfig {
            shadows_dir: std::env::temp_dir().join("iota-disp-unused"),
            ..Default::default()
        };
        let d = Dispatcher::new(cfg);
        assert_eq!(d.active_worker_count(), 0);
    }

    #[test]
    fn recompute_ready_unblocks_tasks() {
        let tmp = tmp_dir();
        let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();

        let board_id = store.create_board("test", "Test Board").unwrap();

        // Create blocker task and advance to Done
        let blocker_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Blocker".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(blocker_id, Status::Todo).unwrap();
        store.transition(blocker_id, Status::Ready).unwrap();
        store.transition(blocker_id, Status::Running).unwrap();
        store.transition(blocker_id, Status::Done).unwrap();

        // Create blocked task and advance to Blocked
        let blocked_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Blocked".to_string(),
                body: None,
                status: None,
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(blocked_id, Status::Todo).unwrap();
        store.transition(blocked_id, Status::Ready).unwrap();
        store.transition(blocked_id, Status::Running).unwrap();
        store.transition(blocked_id, Status::Blocked).unwrap();

        // Create a Blocks link: blocker_id blocks blocked_id
        store
            .create_link(blocker_id, blocked_id, LinkKind::Blocks)
            .unwrap();

        let cfg = DispatcherConfig {
            shadows_dir: tmp.join("shadows"),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::new(cfg);
        dispatcher.recompute_ready(&store).unwrap();

        let task = store.get_task(blocked_id).unwrap();
        assert_eq!(task.status, Status::Ready);
    }

    #[test]
    fn spawn_failure_rolls_task_back_to_ready_and_fails_run() {
        let tmp = tmp_dir();
        let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
        let board_id = store.create_board("test", "Test Board").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Ready task".to_string(),
                body: None,
                status: Some(Status::Ready),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();

        let cfg = DispatcherConfig {
            max_concurrent: 1,
            hermes_bin: PathBuf::from("/missing/hermes-for-iota-test"),
            shadows_dir: tmp.join("shadows"),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::new(cfg);
        let report = dispatcher.tick(&store).unwrap();

        assert_eq!(report.spawn_failures, 1);
        assert_eq!(dispatcher.active_worker_count(), 0);
        assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
        let runs = store.get_runs(task_id).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Failed);
    }

    #[test]
    fn tick_reclaims_expired_running_tasks_without_worker_handle() {
        let tmp = tmp_dir();
        let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
        let board_id = store.create_board("test", "Test Board").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Stale running task".to_string(),
                body: None,
                status: Some(Status::Ready),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(task_id, Status::Running).unwrap();

        let cfg = DispatcherConfig {
            max_concurrent: 0,
            claim_ttl: Duration::from_secs(0),
            shadows_dir: tmp.join("shadows"),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::new(cfg);
        let report = dispatcher.tick(&store).unwrap();

        assert_eq!(report.reclaimed, 1);
        assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    }

    #[test]
    fn reclaim_expired_running_task_closes_stale_running_runs() {
        let tmp = tmp_dir();
        let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
        let board_id = store.create_board("test", "Test Board").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Stale running task".to_string(),
                body: None,
                status: Some(Status::Ready),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(task_id, Status::Running).unwrap();
        let run_id = store.create_run(task_id, "default").unwrap();

        let cfg = DispatcherConfig {
            max_concurrent: 0,
            claim_ttl: Duration::from_secs(0),
            shadows_dir: tmp.join("shadows"),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::new(cfg);
        let report = dispatcher.tick(&store).unwrap();

        assert_eq!(report.reclaimed, 1);
        let runs = store.get_runs(task_id).unwrap();
        let run = runs.iter().find(|run| run.id == run_id).unwrap();
        assert_eq!(run.status, RunStatus::TimedOut);
        assert_eq!(store.get_task(task_id).unwrap().status, Status::Ready);
    }

    #[test]
    fn terminal_shadow_status_updates_task_and_stops_live_worker() {
        let tmp = tmp_dir();
        let store = SqliteKanbanStore::open(&tmp.join("store.db")).unwrap();
        let board_id = store.create_board("test", "Test Board").unwrap();
        let task_id = store
            .create_task(CreateTaskRequest {
                board_id,
                title: "Running task".to_string(),
                body: None,
                status: Some(Status::Ready),
                assignee: None,
                priority: None,
                tags: vec![],
                workspace_kind: None,
                workspace_path: None,
            })
            .unwrap();
        store.transition(task_id, Status::Running).unwrap();
        let run_id = store.create_run(task_id, "test-profile").unwrap();

        let shadow_dir = tmp.join("shadows").join(task_id.to_string());
        std::fs::create_dir_all(&shadow_dir).unwrap();
        let shadow_path = shadow_dir.join("kanban.db");
        let conn = rusqlite::Connection::open(&shadow_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE task_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL
             );
             CREATE TABLE tasks (
                id INTEGER PRIMARY KEY,
                board_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                tags TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                claim_ttl_secs INTEGER NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, board_id, title, status, tags,
             created_at, updated_at, claim_ttl_secs)
             VALUES (?1, ?2, 'test', 'done', '[]', 1, 1, 900)",
            rusqlite::params![task_id as i64, board_id as i64],
        )
        .unwrap();
        drop(conn);

        let child = if cfg!(windows) {
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", "Start-Sleep -Seconds 30"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("sh")
                .args(["-c", "sleep 30"])
                .spawn()
                .unwrap()
        };

        let cfg = DispatcherConfig {
            max_concurrent: 1,
            shadows_dir: tmp.join("shadows"),
            ..Default::default()
        };
        let mut dispatcher = Dispatcher::new(cfg);
        dispatcher.workers.insert(
            task_id,
            (
                WorkerHandle {
                    run_id: run_id.clone(),
                    child,
                    started_at: std::time::Instant::now(),
                },
                ShadowWatcher::new(shadow_path, task_id),
            ),
        );

        let report = dispatcher.tick(&store).unwrap();

        assert_eq!(report.completed, 1);
        assert_eq!(dispatcher.active_worker_count(), 0);
        assert_eq!(store.get_task(task_id).unwrap().status, Status::Done);
        assert_eq!(
            store.get_runs(task_id).unwrap()[0].status,
            RunStatus::Completed
        );
    }
}
