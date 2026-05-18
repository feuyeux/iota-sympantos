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
    pub run_id: RunId,
    pub child: Child,
    pub started_at: Instant,
}

impl WorkerHandle {
    /// Spawn `hermes -p <profile>` with the required environment variables.
    /// The context string is written to stdin.
    pub fn spawn(config: &WorkerConfig, env: WorkerEnv, context: &str) -> Result<Self> {
        let mut command = Command::new(&config.hermes_bin);
        command
            .args(["-p", &env.profile])
            .env("HERMES_KANBAN_TASK", env.task_id.to_string())
            .env("HERMES_KANBAN_RUN_ID", &env.run_id)
            .env(
                "HERMES_KANBAN_DB",
                env.shadow_path.to_string_lossy().as_ref(),
            )
            .env("HERMES_KANBAN_BOARD", &env.board_slug)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_tree_root(&mut command);
        let mut child = command.spawn().context("spawning hermes process")?;

        write_context_or_kill(&mut child, context, write_child_stdin)?;

        Ok(Self {
            run_id: env.run_id,
            child,
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
        kill_process_tree(&mut self.child).context("killing hermes process")
    }

    /// Seconds since the worker was spawned.
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

fn configure_process_tree_root(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn kill_process_tree(child: &mut Child) -> Result<()> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("running taskkill")?;
        if !status.success() {
            child.kill()?;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        child.kill()?;
        Ok(())
    }
}

fn write_context_or_kill(
    child: &mut Child,
    context: &str,
    write_context: impl FnOnce(&mut Child, &str) -> std::io::Result<()>,
) -> Result<()> {
    if let Err(err) = write_context(child, context) {
        let _ = kill_process_tree(child);
        let _ = child.wait();
        return Err(err).context("writing context to hermes stdin");
    }
    Ok(())
}

fn write_child_stdin(child: &mut Child, context: &str) -> std::io::Result<()> {
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(context.as_bytes())?;
    }
    Ok(())
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

    #[test]
    fn kill_stops_child_process_tree() {
        let tmp = std::env::temp_dir().join(format!("iota-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let child_pid_path = tmp.join("child.pid");

        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "powershell",
                vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    format!(
                        "$p = Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 30' -PassThru; Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                        child_pid_path.display()
                    ),
                ],
            )
        } else {
            (
                "sh",
                vec![
                    "-c".into(),
                    format!("sleep 30 & echo $! > '{}'; wait", child_pid_path.display()),
                ],
            )
        };

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_tree_root(&mut command);
        let child = command.spawn().unwrap();
        let mut handle = WorkerHandle {
            run_id: "run".to_string(),
            child,
            started_at: Instant::now(),
        };

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !child_pid_path.exists() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(child_pid_path.exists(), "child pid file was not written");
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .unwrap()
            .trim()
            .to_string();

        handle.kill().unwrap();
        let _ = handle.child.wait();
        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(
            !process_exists(&child_pid),
            "worker kill should terminate descendant process {}",
            child_pid
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn spawn_write_failure_kills_process_tree() {
        let tmp = std::env::temp_dir().join(format!("iota-worker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let child_pid_path = tmp.join("child.pid");

        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "powershell",
                vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    format!(
                        "$p = Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 30' -PassThru; Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                        child_pid_path.display()
                    ),
                ],
            )
        } else {
            (
                "sh",
                vec![
                    "-c".into(),
                    format!("sleep 30 & echo $! > '{}'; wait", child_pid_path.display()),
                ],
            )
        };

        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_tree_root(&mut command);
        let mut child = command.spawn().unwrap();

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !child_pid_path.exists() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(child_pid_path.exists(), "child pid file was not written");
        let child_pid = std::fs::read_to_string(&child_pid_path)
            .unwrap()
            .trim()
            .to_string();

        let result = write_context_or_kill(&mut child, "x", |_child, _context| {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test write failure",
            ))
        });

        assert!(result.is_err());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            !process_exists(&child_pid),
            "write failure cleanup should terminate descendant process {}",
            child_pid
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn process_exists(pid: &str) -> bool {
        if cfg!(windows) {
            std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!("if (Get-Process -Id {} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}", pid),
                ])
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        } else {
            std::process::Command::new("kill")
                .args(["-0", pid])
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        }
    }
}
