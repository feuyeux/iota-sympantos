use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
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

        self.recompute_ready(store)?;

        // Spawn new workers for Ready tasks up to the concurrency limit
        let available = self.config.max_concurrent.saturating_sub(self.workers.len());
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
                let _ = entry.1.sync_events(&events, store, &run_id);
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
                let rs = if exit_code.map(|c| c != 0).unwrap_or(false) {
                    RunStatus::Failed
                } else {
                    RunStatus::Completed
                };
                let _ = store.complete_run(&run_id, rs, exit_code);
                if exit_code.map(|c| c != 0).unwrap_or(false) {
                    // Non-zero exit: transition task back to Ready for retry
                    let _ = store.transition(task_id, Status::Ready);
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

    // -----------------------------------------------------------------------
    // spawn_worker
    // -----------------------------------------------------------------------

    fn spawn_worker(&mut self, task: &Task, store: &dyn KanbanStore) -> Result<()> {
        let profile = task
            .assignee
            .as_deref()
            .unwrap_or("default")
            .to_string();

        // Transition to Running
        store.transition(task.id, Status::Running)?;

        // Create a run record
        let run_id = store.create_run(task.id, &profile)?;

        // Locate the board
        let boards = store.list_boards()?;
        let board = boards
            .into_iter()
            .find(|b| b.id == task.board_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "board {} not found for task {}",
                    task.board_id,
                    task.id
                )
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

        self.workers.insert(task.id, (handle, watcher));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sqlite_store::SqliteKanbanStore;
    use super::super::store::KanbanStore;

    fn tmp_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("iota-disp-{}", uuid::Uuid::new_v4()));
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
}
