use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use super::types::*;

// ---------------------------------------------------------------------------
// WorkerConfig
// ---------------------------------------------------------------------------

pub struct WorkerConfig {
    pub hermes_bin: PathBuf,
}

// ---------------------------------------------------------------------------
// WorkerEnv
// ---------------------------------------------------------------------------

pub struct WorkerEnv {
    pub task_id: TaskId,
    pub run_id: String,
    pub shadow_path: PathBuf,
    pub board_slug: String,
    pub profile: String,
}

// ---------------------------------------------------------------------------
// WorkerHandle
// ---------------------------------------------------------------------------

pub struct WorkerHandle {
    pub task_id: TaskId,
    pub run_id: RunId,
    pub child: Child,
    pub shadow_path: PathBuf,
    pub started_at: Instant,
}

impl WorkerHandle {
    /// Spawn `hermes -p <profile>` with the required environment variables.
    /// The context string is written to stdin.
    pub fn spawn(config: &WorkerConfig, env: WorkerEnv, context: &str) -> Result<Self> {
        let mut child = Command::new(&config.hermes_bin)
            .args(["-p", &env.profile])
            .env("HERMES_KANBAN_TASK", env.task_id.to_string())
            .env("HERMES_KANBAN_RUN_ID", &env.run_id)
            .env("HERMES_KANBAN_DB", env.shadow_path.to_string_lossy().as_ref())
            .env("HERMES_KANBAN_BOARD", &env.board_slug)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawning hermes process")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(context.as_bytes())
                .context("writing context to hermes stdin")?;
        }

        Ok(Self {
            task_id: env.task_id,
            run_id: env.run_id,
            child,
            shadow_path: env.shadow_path,
            started_at: Instant::now(),
        })
    }

    /// Returns the exit code if the child has finished, or `None` if still running.
    pub fn is_finished(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(-1)),
            _ => None,
        }
    }

    /// Kill the child process.
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("killing hermes process")
    }

    /// Seconds since the worker was spawned.
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

// ---------------------------------------------------------------------------
// build_worker_context
// ---------------------------------------------------------------------------

/// Build a markdown context string for hermes, including the task details,
/// any prior runs, and any comments.
pub fn build_worker_context(task: &Task, comments: &[Comment], prior_runs: &[Run]) -> String {
    let body = task.body.as_deref().unwrap_or("");
    let mut out = format!("# Task: {}\n\n{}", task.title, body);

    if !prior_runs.is_empty() {
        out.push_str("\n\n## Prior attempts\n");
        for run in prior_runs {
            let summary = run.output_summary.as_deref().unwrap_or("none");
            out.push_str(&format!(
                "- run `{}` (profile: {}, status: {}, summary: {})\n",
                run.id, run.profile, run.status, summary
            ));
        }
    }

    if !comments.is_empty() {
        out.push_str("\n\n## Comments\n");
        for comment in comments {
            out.push_str(&format!("- {}: {}\n", comment.author, comment.body));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: 1,
            board_id: 1,
            title: "Fix the bug".to_string(),
            body: Some("There is a nasty bug to fix.".to_string()),
            status: Status::Ready,
            assignee: None,
            priority: 0,
            tags: vec![],
            workspace_kind: None,
            workspace_path: None,
            created_at: 0,
            updated_at: 0,
            claimed_at: None,
            claim_ttl_secs: 900,
        }
    }

    #[test]
    fn build_context_includes_task_and_comments() {
        let task = make_task();
        let comments = vec![Comment {
            id: 1,
            task_id: 1,
            author: "alice".to_string(),
            body: "Please fix urgently".to_string(),
            created_at: 0,
        }];
        let ctx = build_worker_context(&task, &comments, &[]);
        assert!(ctx.contains("Fix the bug"), "should include title");
        assert!(
            ctx.contains("There is a nasty bug to fix."),
            "should include body"
        );
        assert!(ctx.contains("alice"), "should include comment author");
        assert!(
            ctx.contains("Please fix urgently"),
            "should include comment body"
        );
    }

    #[test]
    fn build_context_includes_prior_runs() {
        let task = make_task();
        let runs = vec![Run {
            id: "run-001".to_string(),
            task_id: 1,
            profile: "default".to_string(),
            status: RunStatus::Failed,
            started_at: 0,
            finished_at: None,
            last_heartbeat: 0,
            exit_code: Some(1),
            output_summary: Some("Hit an error".to_string()),
        }];
        let ctx = build_worker_context(&task, &[], &runs);
        assert!(
            ctx.contains("Prior attempts"),
            "should include prior attempts section"
        );
        assert!(ctx.contains("run-001"), "should include run id");
        assert!(ctx.contains("default"), "should include profile");
        assert!(ctx.contains("failed"), "should include status");
        assert!(ctx.contains("Hit an error"), "should include summary");
    }
}
