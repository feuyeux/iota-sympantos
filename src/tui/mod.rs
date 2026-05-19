//! iota TUI — ratatui-based interactive chat surface.
//!
//! Layout (top to bottom):
//!   header    1 row  — magenta background, cwd + active backend
//!   history   fill   — scrollable conversation transcript
//!   composer  3 rows — single-line input with history recall
//!   status    1 row  — bottom-left: backend · model  /  right: key hints

mod events;
mod input;
mod kanban_command;
mod kanban_view;
mod r#loop;
mod markdown;
mod render;
mod scrollback;
mod slash_command;
mod state;
mod status_bar;
mod terminal_lifecycle;
mod theme;

#[cfg(test)]
mod slash_command_tests;

use std::io::{IsTerminal, Stdout};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::CrosstermBackend;
use ratatui::{TerminalOptions, Viewport};
use tokio::sync::broadcast as tokio_broadcast;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio::task::JoinHandle;

use crate::acp::permission::{ApprovalRequest, install_tui_approval_channel};
use crate::acp::{ALL_BACKENDS, AcpBackend};
use crate::config::{NimiaConfig, backend_config, configured_model};
use crate::engine::IotaEngine;
use crate::kanban::{AdvancedBridge, Dispatcher, DispatcherConfig, KanbanStore, SqliteKanbanStore};
use crate::telemetry::metrics;
use input::Composer;
use kanban_view::KanbanViewState;
use render::observability_line;
use slash_command::{SlashAction, parse_slash_command, slash_completions};
use state::{ConversationEntry, HistoryState, ObservabilityMeta};
use terminal_lifecycle::{TerminalGuard, install_terminal_panic_hook};

type Terminal = ratatui::Terminal<CrosstermBackend<Stdout>>;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_HISTORY: usize = 500;
/// Inline viewport rows: optional Kanban panel + spinner + composer + status bar.
const VIEWPORT_HEIGHT: u16 = 18;

// ── Typed turn message ────────────────────────────────────────────────────────

/// Messages sent from the submit path to the engine dispatch loop.
#[derive(Debug)]
enum TurnMessage {
    Prompt {
        backend: AcpBackend,
        cwd: PathBuf,
        text: String,
    },
}

// ── Overlay mode ─────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Overlay {
    None,
    QuitConfirm,
}

// ── TuiApp ──────────────────────────────────────────────────────────────────

struct TuiApp {
    // Core runtime context
    /// Engine is wrapped in Arc<TokioMutex> so engine calls can be spawned
    /// on a separate task without blocking the TUI event loop.
    engine: Arc<TokioMutex<IotaEngine>>,
    config: NimiaConfig,
    cwd: PathBuf,

    // Kanban store
    kanban_store: Arc<dyn KanbanStore>,
    kanban_dispatcher: Arc<Mutex<Dispatcher>>,
    kanban_bridge: AdvancedBridge,
    kanban_view: KanbanViewState,
    /// Whether auto-dispatch daemon is active (background task ticks the dispatcher).
    kanban_daemon_active: Arc<AtomicBool>,
    /// Broadcast receiver for real-time kanban UI events.
    kanban_event_rx: tokio_broadcast::Receiver<crate::kanban::KanbanUiEvent>,

    // Conversation and input state
    history: HistoryState,
    composer: Composer,

    // Active backend/model selection
    active_backend: AcpBackend,
    active_model: Option<String>,

    // Turn lifecycle state
    running_turn: bool,
    tick_count: u64,
    /// Currently running engine task, if a turn is active.
    turn_task: Option<JoinHandle<()>>,
    /// When running_turn is true, when did it start (for elapsed display).
    turn_started_at: Option<std::time::Instant>,
    /// Queued prompt while a turn is running (Tab to queue).
    queued_prompt: Option<String>,

    // Engine dispatch channels
    /// Typed channel for submitting prompts to the engine dispatch task.
    turn_tx: mpsc::Sender<TurnMessage>,
    turn_rx: mpsc::Receiver<TurnMessage>,

    // Approval and observability
    /// Pending approval request from the ACP layer (shown as an overlay).
    pending_approval: Option<ApprovalRequest>,
    latest_observability: Option<ObservabilityMeta>,

    // Streaming output state
    /// Streaming output channel — receives partial chunks while engine runs.
    stream_rx: mpsc::Receiver<String>,
    /// Sender side kept so we can re-install it on the engine each turn.
    stream_tx: mpsc::Sender<String>,
    /// Accumulated streamed text for the current in-progress turn.
    streaming_text: String,
    /// Monotonically incremented each time `streaming_text` is mutated.
    streaming_version: std::cell::Cell<u64>,
    /// Backend for the current in-progress streaming turn (for display label).
    streaming_backend: Option<AcpBackend>,

    // Overlay/ui flags
    /// Active overlay (quit-confirm only in inline mode).
    overlay: Overlay,
    /// Quit confirmation: tick when first Ctrl+C was pressed.
    quit_confirm_tick: Option<u64>,
    /// Entries waiting to be inserted into terminal scrollback.
    pending_scrollback_entries: Vec<ConversationEntry>,
    /// Deferred help rendering request handled by the loop with terminal access.
    help_requested: bool,
}

impl TuiApp {
    // ── construction ─────────────────────────────────────────────────────────

    fn new(config: NimiaConfig) -> Result<Self> {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;

        // Pick the first enabled backend as the default.
        let active_backend = ALL_BACKENDS
            .iter()
            .copied()
            .find(|&b| {
                backend_config(&config, b)
                    .map(|c| c.enabled)
                    .unwrap_or(false)
            })
            .unwrap_or(AcpBackend::Codex);

        let active_model = backend_config(&config, active_backend).and_then(configured_model);

        let engine = Arc::new(TokioMutex::new(IotaEngine::new(
            config.clone(),
            false,
            300_000, // 5 minutes timeout for TUI
        )));
        let (turn_tx, turn_rx) = mpsc::channel(4);
        let (stream_tx, stream_rx) = mpsc::channel::<String>(64);

        // Kanban store initialization
        let kanban_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".i6")
            .join("kanban");
        std::fs::create_dir_all(&kanban_dir).ok();
        let kanban_db_path = kanban_dir.join("iota.db");
        let kanban_store_concrete =
            SqliteKanbanStore::open(&kanban_db_path).context("Failed to open kanban store")?;
        let kanban_event_rx = kanban_store_concrete.subscribe();
        let kanban_store: Arc<dyn KanbanStore> = Arc::new(kanban_store_concrete);
        let dispatcher_config = DispatcherConfig::default();
        let kanban_bridge = AdvancedBridge::new(
            dispatcher_config.hermes_bin.clone(),
            dispatcher_config.shadows_dir.clone(),
        );
        let kanban_dispatcher = Arc::new(Mutex::new(Dispatcher::new(dispatcher_config)));
        let kanban_daemon_active = Arc::new(AtomicBool::new(false));

        Ok(Self {
            engine,
            config,
            cwd,
            kanban_store,
            kanban_dispatcher,
            kanban_bridge,
            kanban_view: KanbanViewState::default(),
            kanban_daemon_active,
            kanban_event_rx,
            history: HistoryState::new(MAX_HISTORY),
            composer: Composer::new(),
            active_backend,
            active_model,
            running_turn: false,
            tick_count: 0,
            turn_tx,
            turn_rx,
            pending_approval: None,
            latest_observability: None,
            stream_rx,
            stream_tx,
            streaming_text: String::new(),
            streaming_version: std::cell::Cell::new(0),
            streaming_backend: None,
            overlay: Overlay::None,
            turn_task: None,
            turn_started_at: None,
            queued_prompt: None,
            quit_confirm_tick: None,
            pending_scrollback_entries: Vec::new(),
            help_requested: false,
        })
    }

    // ── backend management ───────────────────────────────────────────────────

    fn enabled_backends(&self) -> Vec<AcpBackend> {
        ALL_BACKENDS
            .iter()
            .copied()
            .filter(|&b| {
                backend_config(&self.config, b)
                    .map(|c| c.enabled)
                    .unwrap_or(false)
            })
            .collect()
    }

    fn switch_backend(&mut self, backend: AcpBackend) {
        self.active_backend = backend;
        self.active_model = backend_config(&self.config, backend).and_then(configured_model);

        let notice = format!(
            "Switched to {} · {}",
            backend,
            self.active_model.as_deref().unwrap_or("—")
        );
        self.record_entry(ConversationEntry::SystemNotice { text: notice });
    }

    fn try_switch_backend_from_slash_command(&mut self, backend: AcpBackend) {
        if !self.enabled_backends().contains(&backend) {
            self.record_entry(ConversationEntry::SystemNotice {
                text: format!("Backend {} is disabled in ~/.i6/nimia.yaml", backend),
            });
            return;
        }
        self.switch_backend(backend);
    }

    fn handle_slash_command(&mut self, text: &str) -> bool {
        if !text.starts_with('/') {
            return false;
        }

        let Some(command) = parse_slash_command(text, self.active_backend) else {
            return false;
        };

        match command.action {
            SlashAction::SubmitToBackend => {
                return false;
            }
            SlashAction::Help => {
                self.help_requested = true;
            }
            SlashAction::Clear => {
                self.history.entries.clear();
                self.pending_scrollback_entries.clear();
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Transcript cache cleared (terminal scrollback preserved)".into(),
                });
            }
            SlashAction::ListBackends => {
                let enabled = self
                    .enabled_backends()
                    .into_iter()
                    .map(|backend| {
                        if backend == self.active_backend {
                            format!("{}*", backend)
                        } else {
                            backend.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!("Enabled backends: {}", enabled),
                });
            }
            SlashAction::SwitchBackend(backend) => {
                self.try_switch_backend_from_slash_command(backend);
            }
            SlashAction::Model => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!(
                        "{} model: {}",
                        self.active_backend,
                        self.active_model.as_deref().unwrap_or("—")
                    ),
                });
            }
            SlashAction::Status => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!(
                        "Status: backend {} · model {}",
                        self.active_backend,
                        self.active_model.as_deref().unwrap_or("—"),
                    ),
                });
            }
            SlashAction::Export => match self.export_history() {
                Ok(path) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Exported to {}", path.display()),
                    });
                }
                Err(err) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Export failed: {}", err),
                    });
                }
            },
            SlashAction::Kanban => {
                if self.handle_kanban_view_slash(command.args) {
                    return true;
                }
                let lines = kanban_command::execute_with_services(
                    command.args,
                    &self.kanban_store,
                    None,
                    Some(&self.kanban_dispatcher),
                    Some(&self.kanban_daemon_active),
                    Some(&self.kanban_bridge),
                );
                for line in lines {
                    self.record_entry(ConversationEntry::SystemNotice { text: line });
                }
            }
            SlashAction::Quit => {
                self.quit_confirm_tick = Some(self.tick_count);
                self.overlay = Overlay::QuitConfirm;
            }
        }
        true
    }

    fn handle_kanban_view_slash(&mut self, args: &str) -> bool {
        let parts: Vec<&str> = args.split_whitespace().collect();
        let subcmd = parts.first().copied().unwrap_or("");
        match subcmd {
            "tab" | "ui" | "open" => {
                let board_slug = parts.get(1).map(|slug| (*slug).to_string());
                self.kanban_view.open(board_slug);
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Kanban tab opened. Keys: 1-4/L/R modes, j/k select, Tab columns, Enter detail, n/e/c/m/a, d/D, / filter, Esc close.".into(),
                });
                true
            }
            "close" => {
                self.kanban_view.close();
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Kanban tab closed.".into(),
                });
                true
            }
            "filter" => {
                self.kanban_view.set_filter(parts[1..].join(" "));
                self.record_entry(ConversationEntry::SystemNotice {
                    text: if self.kanban_view.filter.is_empty() {
                        "Kanban filter cleared.".into()
                    } else {
                        format!("Kanban filter: {}", self.kanban_view.filter)
                    },
                });
                true
            }
            _ => false,
        }
    }

    fn execute_kanban_command(&mut self, args: &str) {
        let lines = kanban_command::execute_with_services(
            args,
            &self.kanban_store,
            None,
            Some(&self.kanban_dispatcher),
            Some(&self.kanban_daemon_active),
            Some(&self.kanban_bridge),
        );
        for line in lines {
            self.record_entry(ConversationEntry::SystemNotice { text: line });
        }
    }

    // ── slash completion ─────────────────────────────────────────────────────

    /// Returns the ghost-text suffix to display after the cursor when typing a
    /// slash command. For example, `/he` on Hermes returns `Some("lp")` so the
    /// render layer can append it in dim gray.
    pub(super) fn slash_ghost(&self) -> Option<String> {
        let text = &self.composer.text;
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return None;
        }
        if !self.composer.cursor_at_end() {
            return None;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, self.active_backend);
        let first = completions.first()?;
        if first.len() > prefix.len() {
            Some(first[prefix.len()..].to_string())
        } else {
            None
        }
    }

    /// Returns a space-separated list of matching slash command names for the
    /// composer border title. Returns `None` when not in slash-typing mode.
    pub(super) fn slash_completion_hint(&self) -> Option<String> {
        let text = &self.composer.text;
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return None;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, self.active_backend);
        if completions.is_empty() {
            return None;
        }
        const MAX_SHOW: usize = 8;
        let shown: Vec<&str> = completions.iter().copied().take(MAX_SHOW).collect();
        let mut hint = shown.join("  ");
        if completions.len() > MAX_SHOW {
            hint.push_str(&format!("  +{}", completions.len() - MAX_SHOW));
        }
        Some(hint)
    }

    /// Accept the first slash-command completion by replacing the composer text.
    /// Returns `true` if a completion was accepted.
    pub(super) fn slash_tab_complete(&mut self) -> bool {
        let text = self.composer.text.clone();
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return false;
        }
        if !self.composer.cursor_at_end() {
            return false;
        }
        let prefix = &text[1..];
        let completions = slash_completions(prefix, self.active_backend);
        if let Some(&first) = completions.first() {
            if first != prefix {
                let completed = format!("/{}", first);
                self.composer.cursor = completed.chars().count();
                self.composer.text = completed;
                return true;
            }
        }
        false
    }

    fn cycle_backend(&mut self) {
        let enabled = self.enabled_backends();
        if enabled.is_empty() {
            return;
        }
        let idx = enabled
            .iter()
            .position(|&b| b == self.active_backend)
            .unwrap_or(0);
        let next = enabled[(idx + 1) % enabled.len()];
        self.switch_backend(next);
    }

    fn record_entry(&mut self, entry: ConversationEntry) {
        self.history.push(entry.clone());
        self.pending_scrollback_entries.push(entry);
    }

    // ── export ───────────────────────────────────────────────────────────────

    fn export_history(&mut self) -> Result<PathBuf> {
        use std::fs;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("iota_transcript_{}.txt", timestamp);
        let path = self.cwd.join(&filename);

        let mut content = String::new();
        content.push_str("iota TUI Transcript\n");
        content.push_str(&format!(
            "Exported: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        content.push_str(&format!("Backend: {}\n", self.active_backend));
        if let Some(model) = &self.active_model {
            content.push_str(&format!("Model: {}\n", model));
        }
        content.push_str(&format!("Working Directory: {}\n", self.cwd.display()));
        content.push('\n');
        content.push_str(&"=".repeat(80));
        content.push_str("\n\n");

        for entry in &self.history.entries {
            match entry {
                ConversationEntry::UserMessage { text, .. } => {
                    content.push_str("YOU:\n");
                    content.push_str(text);
                    content.push_str("\n\n");
                }
                ConversationEntry::AssistantMessage {
                    backend,
                    text,
                    observability,
                } => {
                    content.push_str(&format!("{}:\n", backend.to_string().to_uppercase()));
                    content.push_str(text);
                    content.push('\n');
                    if let Some(meta) = observability
                        && let Some(line) = observability_line(meta, None)
                    {
                        content.push_str(&format!("[{}]\n", line));
                    }
                    content.push('\n');
                }
                ConversationEntry::SystemNotice { text } => {
                    content.push_str(&format!("── {} ──\n\n", text));
                }
                ConversationEntry::ToolResult { name, ok, text } => {
                    let icon = if *ok { "✓" } else { "✗" };
                    content.push_str(&format!("{} {} → {}\n\n", icon, name, text));
                }
            }
        }

        fs::write(&path, content)?;
        Ok(path)
    }

    // ── submit ───────────────────────────────────────────────────────────────

    fn submit(&mut self) {
        let text = self.composer.take_submit();
        if text.is_empty() {
            return;
        }
        // Resolve alias → canonical name for SubmitToBackend commands.
        // e.g. user types "/compress" → we forward "/compact" so the backend's
        // ACP slash handler sees the canonical name it registers against.
        let forward_text: Option<String> = parse_slash_command(&text, self.active_backend)
            .filter(|cmd| cmd.action == SlashAction::SubmitToBackend)
            .map(|cmd| {
                if cmd.args.is_empty() {
                    format!("/{}", cmd.name)
                } else {
                    format!("/{} {}", cmd.name, cmd.args)
                }
            });
        let forward_text = forward_text.unwrap_or_else(|| text.clone());

        if self.handle_slash_command(&text) {
            return;
        }
        if self.running_turn {
            // Tab-queue: store for after current turn finishes
            self.queued_prompt = Some(forward_text);
            self.record_queued_prompt_delta(1);
            self.record_entry(ConversationEntry::SystemNotice {
                text: "Queued (will send after current turn)".into(),
            });
            return;
        }
        self.record_entry(ConversationEntry::UserMessage {
            text: forward_text.clone(),
            backend: Some(self.active_backend),
        });
        self.running_turn = true;
        self.turn_started_at = Some(std::time::Instant::now());
        self.send_turn_prompt(self.active_backend, self.cwd.clone(), forward_text);
    }

    fn record_queued_prompt_delta(&self, delta: i64) {
        metrics::get().prompt_queued.add(delta, &[]);
    }

    // ── engine dispatch ─────────────────────────────────────────────────────

    fn send_turn_prompt(&mut self, backend: AcpBackend, cwd: PathBuf, text: String) {
        let tx = self.turn_tx.clone();
        match tx.try_send(TurnMessage::Prompt { backend, cwd, text }) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
                tracing::warn!(
                    backend = %self.active_backend,
                    "turn channel full; retrying via async send"
                );
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    let _ = tx2.send(msg).await;
                });
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!("turn channel closed; engine has shut down");
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Error: engine channel closed".into(),
                });
                self.running_turn = false;
            }
        }
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

// ── run bootstrap ───────────────────────────────────────────────────────────

pub async fn run(config: NimiaConfig) -> Result<()> {
    // Ensure stdout is a real terminal before entering raw mode.
    if !std::io::stdout().is_terminal() {
        anyhow::bail!("stdout is not a terminal — cannot start TUI");
    }

    let mut app = TuiApp::new(config)?;

    // Install the approval channel so acp.rs routes permission requests here.
    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(8);
    install_tui_approval_channel(approval_tx).await;

    install_terminal_panic_hook();

    // Terminal setup — inline viewport, no alt-screen, no mouse capture.
    // This lets the terminal own scrollback (native scroll/copy/selection),
    // mirroring codex's TUI architecture.
    enable_raw_mode()?;
    let stdout = std::io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(VIEWPORT_HEIGHT),
        },
    )?;

    // The guard ensures teardown on all exit paths (including `?` propagation).
    let _guard = TerminalGuard;

    // Emit the iota banner once so it lives in normal terminal scrollback.
    let _ = scrollback::insert_lines(&mut terminal, scrollback::banner_lines());

    let result = r#loop::run_loop(&mut terminal, &mut app, approval_rx).await;

    result
}
