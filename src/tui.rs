//! iota TUI — ratatui-based interactive chat surface.
//!
//! Layout (top to bottom):
//!   header    1 row  — magenta background, cwd + active backend
//!   history   fill   — scrollable conversation transcript
//!   composer  3 rows — single-line input with history recall
//!   status    1 row  — bottom-left: backend · model  /  right: key hints

pub mod composer;
pub mod markdown;
pub mod state;
pub mod status_bar;
pub mod theme;

use std::io::{IsTerminal, Stdout};
use std::path::PathBuf;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event as CEvent, EventStream, KeyCode, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::acp::{ALL_BACKENDS, AcpBackend, ApprovalRequest, install_tui_approval_channel};
use crate::config::{NimiaConfig, backend_config, configured_model};
use crate::engine::IotaEngine;
use composer::{Composer, ComposerAction};
use state::{ConversationEntry, HistoryState};

type Terminal = ratatui::Terminal<CrosstermBackend<Stdout>>;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_HISTORY: usize = 500;

// ── Overlay mode ─────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Pager { scroll: usize },
    QuitConfirm,
}

// ── TuiApp ──────────────────────────────────────────────────────────────────

struct TuiApp {
    engine: IotaEngine,
    config: NimiaConfig,
    cwd: PathBuf,
    history: HistoryState,
    composer: Composer,
    active_backend: AcpBackend,
    active_model: Option<String>,
    running_turn: bool,
    tick_count: u64,
    /// Result sender — the spawned task posts its result here.
    turn_tx: mpsc::Sender<Result<String>>,
    turn_rx: mpsc::Receiver<Result<String>>,
    /// Pending approval request from the ACP layer (shown as an overlay).
    pending_approval: Option<ApprovalRequest>,
    /// Streaming output channel — receives partial chunks while engine runs.
    stream_rx: mpsc::Receiver<String>,
    /// Sender side kept so we can re-install it on the engine each turn.
    stream_tx: mpsc::Sender<String>,
    /// Accumulated streamed text for the current in-progress turn.
    streaming_text: String,
    /// Backend for the current in-progress streaming turn (for display label).
    streaming_backend: Option<AcpBackend>,
    /// Active overlay (help / pager / quit-confirm).
    overlay: Overlay,
    /// When running_turn is true, when did it start (for elapsed display).
    turn_started_at: Option<std::time::Instant>,
    /// Queued prompt while a turn is running (Tab to queue).
    queued_prompt: Option<String>,
    /// Quit confirmation: tick when first Ctrl+C was pressed.
    quit_confirm_tick: Option<u64>,
}

impl TuiApp {
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

        let engine = IotaEngine::new(config.clone(), false, crate::acp::DEFAULT_TIMEOUT_MS);
        let (turn_tx, turn_rx) = mpsc::channel(4);
        let (stream_tx, stream_rx) = mpsc::channel::<String>(64);

        Ok(Self {
            engine,
            config,
            cwd,
            history: HistoryState::new(MAX_HISTORY),
            composer: Composer::new(),
            active_backend,
            active_model,
            running_turn: false,
            tick_count: 0,
            turn_tx,
            turn_rx,
            pending_approval: None,
            stream_rx,
            stream_tx,
            streaming_text: String::new(),
            streaming_backend: None,
            overlay: Overlay::None,
            turn_started_at: None,
            queued_prompt: None,
            quit_confirm_tick: None,
        })
    }

    // ── backend cycling (Ctrl+B) ─────────────────────────────────────────────

    fn cycle_backend(&mut self) {
        let config = &self.config;
        let enabled: Vec<AcpBackend> = ALL_BACKENDS
            .iter()
            .copied()
            .filter(|&b| {
                backend_config(config, b)
                    .map(|c| c.enabled)
                    .unwrap_or(false)
            })
            .collect();

        if enabled.len() <= 1 {
            return;
        }

        let current = self.active_backend;
        let next = enabled
            .iter()
            .copied()
            .skip_while(|&b| b != current)
            .nth(1)
            .or_else(|| enabled.first().copied())
            .unwrap_or(current);

        if next == current {
            return;
        }

        self.active_backend = next;
        self.active_model = backend_config(config, next).and_then(configured_model);

        let notice = format!(
            "Switched to {} · {}",
            next,
            self.active_model.as_deref().unwrap_or("—")
        );
        self.history
            .push(ConversationEntry::SystemNotice { text: notice });
        self.history.scroll_to_bottom();
    }

    // ── submit ───────────────────────────────────────────────────────────────

    fn submit(&mut self) {
        let text = self.composer.take_submit();
        if text.is_empty() {
            return;
        }
        if self.running_turn {
            // Tab-queue: store for after current turn finishes
            self.queued_prompt = Some(text);
            self.history.push(ConversationEntry::SystemNotice {
                text: "Queued (will send after current turn)".into(),
            });
            return;
        }
        self.history
            .push(ConversationEntry::UserMessage { text: text.clone() });
        self.history.scroll_to_bottom();
        self.running_turn = true;
        self.turn_started_at = Some(std::time::Instant::now());
        let tx = self.turn_tx.clone();
        let _ = tx.try_send(Err(anyhow::anyhow!("__prompt__:{}", text)));
    }

    // ── render ───────────────────────────────────────────────────────────────

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // If a full-screen overlay is active, render it and return.
        match &self.overlay {
            Overlay::Pager { scroll } => {
                self.render_pager(frame, area, *scroll);
                return;
            }
            Overlay::Help => {
                self.render_help(frame, area);
                return;
            }
            Overlay::QuitConfirm => {
                // render normal UI below; quit-confirm shown in status bar
            }
            Overlay::None => {}
        }

        // Status indicator height: 1 row when running, else 0.
        let si_height = if self.running_turn { 1 } else { 0 };

        let chunks = Layout::vertical([
            Constraint::Length(1),            // header
            Constraint::Fill(1),              // history
            Constraint::Length(si_height),    // status indicator
            Constraint::Length(3),            // composer / approval
            Constraint::Length(1),            // status bar
        ])
        .split(area);

        self.render_header(frame, chunks[0]);
        self.render_history(frame, chunks[1]);
        if self.running_turn {
            self.render_status_indicator(frame, chunks[2]);
        }
        if self.pending_approval.is_some() {
            self.render_approval_overlay(frame, chunks[3]);
        } else {
            self.render_composer(frame, chunks[3]);
        }
        status_bar::render(
            frame,
            chunks[4],
            self.active_backend,
            self.active_model.as_deref(),
            self.composer.is_searching(),
            self.composer.search_query(),
            self.running_turn,
            self.queued_prompt.is_some(),
            matches!(self.overlay, Overlay::QuitConfirm),
        );
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let cwd_str = self.cwd.display().to_string();
        let backend_str = self.active_backend.to_string();
        let text = format!(" iota  {}  [{}]", cwd_str, backend_str);
        let line = Line::from(Span::styled(text, theme::header_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_history(&self, frame: &mut Frame, area: Rect) {
        let entries = &self.history.entries;
        let mut lines: Vec<Line> = Vec::new();

        for entry in entries.iter() {
            match entry {
                ConversationEntry::UserMessage { text } => {
                    lines.push(Line::from(vec![
                        Span::styled("you  ", theme::user_label_style()),
                        Span::raw(text.clone()),
                    ]));
                    lines.push(Line::raw(""));
                }
                ConversationEntry::AssistantMessage { backend, text } => {
                    lines.push(Line::from(Span::styled(
                        format!("{}  ", backend),
                        theme::assistant_label_style(),
                    )));
                    for md_line in markdown::render(text) {
                        let indented = Line::from(
                            std::iter::once(Span::raw("     "))
                                .chain(md_line.spans)
                                .collect::<Vec<_>>(),
                        );
                        lines.push(indented);
                    }
                    lines.push(Line::raw(""));
                }
                ConversationEntry::SystemNotice { text } => {
                    lines.push(Line::from(Span::styled(
                        format!("── {} ──", text),
                        theme::system_notice_style(),
                    )));
                    lines.push(Line::raw(""));
                }
                ConversationEntry::ToolCall { name, args } => {
                    lines.push(Line::from(vec![
                        Span::styled("⚙ ", theme::tool_call_style()),
                        Span::styled(format!("{}({})", name, args), theme::tool_call_style()),
                    ]));
                }
                ConversationEntry::ToolResult { name, ok, text } => {
                    let style = if *ok {
                        theme::tool_result_ok_style()
                    } else {
                        theme::tool_result_err_style()
                    };
                    let icon = if *ok { "✓" } else { "✗" };
                    lines.push(Line::from(Span::styled(
                        format!("{} {} → {}", icon, name, text),
                        style,
                    )));
                }
            }
        }

        // Live streaming text while engine responds.
        if self.running_turn && !self.streaming_text.is_empty() {
            let label = self
                .streaming_backend
                .map(|b| format!("{}  ", b))
                .unwrap_or_else(|| "…  ".to_string());
            lines.push(Line::from(Span::styled(label, theme::assistant_label_style())));
            for md_line in markdown::render(&self.streaming_text) {
                let indented = Line::from(
                    std::iter::once(Span::raw("     "))
                        .chain(md_line.spans)
                        .collect::<Vec<_>>(),
                );
                lines.push(indented);
            }
        }

        let total = lines.len() as u16;
        let viewport_height = area.height;
        let max_scroll = total.saturating_sub(viewport_height) as usize;
        let offset = self.history.scroll_offset.min(max_scroll);
        let scroll_row = total.saturating_sub(viewport_height) as usize;
        let effective_offset = scroll_row.saturating_sub(offset);

        let para = Paragraph::new(lines)
            .scroll((effective_offset as u16, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }

    fn render_composer(&self, frame: &mut Frame, area: Rect) {
        let title_hint = if self.composer.is_searching() {
            let q = self.composer.search_query().unwrap_or("");
            format!(" Ctrl+R: {}_ ", q)
        } else if self.running_turn {
            let frame_char = SPINNER_FRAMES[(self.tick_count / 4) as usize % SPINNER_FRAMES.len()];
            format!(" {} ", frame_char)
        } else {
            String::new()
        };

        let title_style = if self.composer.is_searching() {
            theme::system_notice_style()
        } else if self.running_turn {
            theme::spinner_style()
        } else {
            theme::composer_border_style(true)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(
                !self.running_turn && !self.composer.is_searching(),
            ))
            .title(Span::styled(title_hint, title_style));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.composer.text.is_empty() && !self.running_turn {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Type a prompt and press Enter… (? for help)",
                    Style::default().fg(ratatui::style::Color::DarkGray),
                ))),
                inner,
            );
            return;
        }

        // Multi-line: split by newlines and insert cursor marker.
        let (disp_lines, cur_row, cur_col) = self.composer.display_lines();
        let para_lines: Vec<Line> = disp_lines
            .iter()
            .enumerate()
            .map(|(row, line_text)| {
                if row == cur_row {
                    let before: String = line_text
                        .chars()
                        .take(cur_col)
                        .collect();
                    let after: String = line_text
                        .chars()
                        .skip(cur_col)
                        .collect();
                    Line::from(vec![
                        Span::raw(before),
                        Span::styled("│", theme::user_label_style()),
                        Span::raw(after),
                    ])
                } else {
                    Line::raw(line_text.clone())
                }
            })
            .collect();

        // Scroll the paragraph so the cursor row is visible.
        let vp_height = inner.height as usize;
        let scroll_row = if cur_row >= vp_height {
            (cur_row - vp_height + 1) as u16
        } else {
            0
        };

        frame.render_widget(
            Paragraph::new(para_lines)
                .scroll((scroll_row, 0))
                .wrap(Wrap { trim: false }),
            inner,
        );
    }

    fn render_approval_overlay(&self, frame: &mut Frame, area: Rect) {        let tool_name = self
            .pending_approval
            .as_ref()
            .map(|r| r.tool_name.as_str())
            .unwrap_or("tool");

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::tool_call_style())
            .title(Span::styled(
                " Approval Required ",
                theme::tool_call_style(),
            ));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let line = Line::from(vec![
            Span::styled("Allow ", theme::assistant_text_style()),
            Span::styled(tool_name, theme::tool_call_style()),
            Span::styled("?  ", theme::assistant_text_style()),
            Span::styled("[y] ", theme::tool_result_ok_style()),
            Span::styled("approve  ", theme::assistant_text_style()),
            Span::styled("[n] ", theme::tool_result_err_style()),
            Span::styled("deny", theme::assistant_text_style()),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
    }
}

// ── New render helpers ────────────────────────────────────────────────────────

impl TuiApp {
    /// One-row status indicator above the composer while a turn is running.
    fn render_status_indicator(&self, frame: &mut Frame, area: Rect) {
        let elapsed = self
            .turn_started_at
            .map(|t| {
                let s = t.elapsed().as_secs();
                if s < 60 {
                    format!("{}s", s)
                } else {
                    format!("{}m {:02}s", s / 60, s % 60)
                }
            })
            .unwrap_or_default();
        let frame_char = SPINNER_FRAMES[(self.tick_count / 4) as usize % SPINNER_FRAMES.len()];
        let line = Line::from(vec![
            Span::styled(format!(" {} ", frame_char), theme::spinner_style()),
            Span::styled("Working… ", theme::spinner_style()),
            Span::styled(elapsed, theme::status_bar_hint_style()),
            Span::styled("  Esc to interrupt", theme::status_bar_hint_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    /// Full-screen pager (Ctrl+T) showing all history lines.
    fn render_pager(&self, frame: &mut Frame, area: Rect, scroll: usize) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(true))
            .title(Span::styled(
                " Transcript  [q/Ctrl+T close  ↑↓/j/k scroll  Home/End] ",
                theme::status_bar_style(),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        for entry in self.history.entries.iter() {
            match entry {
                ConversationEntry::UserMessage { text } => {
                    lines.push(Line::from(vec![
                        Span::styled("you  ", theme::user_label_style()),
                        Span::raw(text.clone()),
                    ]));
                    lines.push(Line::raw(""));
                }
                ConversationEntry::AssistantMessage { backend, text } => {
                    lines.push(Line::from(Span::styled(
                        format!("{}  ", backend),
                        theme::assistant_label_style(),
                    )));
                    for md_line in markdown::render(text) {
                        let indented = Line::from(
                            std::iter::once(Span::raw("     "))
                                .chain(md_line.spans)
                                .collect::<Vec<_>>(),
                        );
                        lines.push(indented);
                    }
                    lines.push(Line::raw(""));
                }
                ConversationEntry::SystemNotice { text } => {
                    lines.push(Line::from(Span::styled(
                        format!("── {} ──", text),
                        theme::system_notice_style(),
                    )));
                }
                ConversationEntry::ToolCall { name, args } => {
                    lines.push(Line::from(Span::styled(
                        format!("⚙ {}({})", name, args),
                        theme::tool_call_style(),
                    )));
                }
                ConversationEntry::ToolResult { name, ok, text } => {
                    let style = if *ok {
                        theme::tool_result_ok_style()
                    } else {
                        theme::tool_result_err_style()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{} {} → {}", if *ok { "✓" } else { "✗" }, name, text),
                        style,
                    )));
                }
            }
        }

        let total = lines.len();
        let vp = inner.height as usize;
        let max_scroll = total.saturating_sub(vp);
        let offset = scroll.min(max_scroll) as u16;

        frame.render_widget(
            Paragraph::new(lines)
                .scroll((offset, 0))
                .wrap(Wrap { trim: false }),
            inner,
        );
    }

    /// Help overlay (? key).
    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(true))
            .title(Span::styled(" Keyboard Shortcuts ", theme::status_bar_style()));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: &[(&str, &str)] = &[
            ("Enter",        "Send prompt"),
            ("Shift+Enter",  "Insert newline"),
            ("Tab",          "Queue prompt while running"),
            ("↑ / ↓",        "History recall"),
            ("Ctrl+R",       "Search history"),
            ("Ctrl+K",       "Kill to end of line"),
            ("Ctrl+Y",       "Yank kill buffer"),
            ("Ctrl+W",       "Delete word backward"),
            ("Alt+B / Alt+F","Word backward / forward"),
            ("Ctrl+T",       "Toggle transcript pager"),
            ("Ctrl+B",       "Cycle backend"),
            ("Ctrl+L",       "Clear history"),
            ("Esc",          "Interrupt running turn"),
            ("? ",           "Toggle this help"),
            ("Ctrl+C",       "Quit (press twice)"),
        ];

        let lines: Vec<Line> = items
            .iter()
            .map(|(k, v)| {
                Line::from(vec![
                    Span::styled(format!("  {:20}", k), theme::tool_call_style()),
                    Span::styled(*v, theme::assistant_text_style()),
                ])
            })
            .collect();

        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            inner,
        );
    }
}

// ── Terminal cleanup guard ───────────────────────────────────────────────────

/// Restores terminal state on drop so that panics and early returns always
/// leave the terminal in a usable state.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

pub async fn run(config: NimiaConfig) -> Result<()> {
    // Ensure stdout is a real terminal before entering raw mode.
    if !std::io::stdout().is_terminal() {
        anyhow::bail!("stdout is not a terminal — cannot start TUI");
    }

    let mut app = TuiApp::new(config)?;

    // Install the approval channel so acp.rs routes permission requests here.
    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(8);
    install_tui_approval_channel(approval_tx);

    // Install a panic hook that restores the terminal before printing the
    // panic message, so the user's shell is not left in raw mode.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // The guard ensures teardown on all exit paths (including `?` propagation).
    let _guard = TerminalGuard;

    let result = run_loop(&mut terminal, &mut app, approval_rx).await;

    terminal.show_cursor()?;
    // _guard drops here and calls disable_raw_mode + LeaveAlternateScreen.

    result
}

async fn run_loop(
    terminal: &mut Terminal,
    app: &mut TuiApp,
    mut approval_rx: mpsc::Receiver<ApprovalRequest>,
) -> Result<()> {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(80));
    let mut events = EventStream::new();

    // Frame rate limiter — skip redraw if we drew less than MIN_FRAME_MS ago.
    const MIN_FRAME_MS: u64 = 8; // ~120 fps cap
    let mut last_draw = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(MIN_FRAME_MS))
        .unwrap_or(std::time::Instant::now());

    // engine_result carries the completed engine response back to the loop.
    // We use a one-shot channel so the engine future can be driven without
    // holding a mutable borrow on `app` across the terminal.draw() call.
    let (engine_tx, mut engine_rx) = mpsc::channel::<Result<(AcpBackend, String)>>(1);

    // pending_prompt is set by the submit path.
    let mut pending_prompt: Option<(AcpBackend, PathBuf, String)> = None;

    loop {
        let now = std::time::Instant::now();
        if now.duration_since(last_draw).as_millis() as u64 >= MIN_FRAME_MS {
            terminal.draw(|f| app.render(f))?;
            last_draw = now;
        }

        // Kick off the engine call as a local task on the first loop iteration
        // after a prompt is queued, so it doesn't block draw/event handling.
        if let Some((backend, cwd, prompt)) = pending_prompt.take() {
            // Install the streaming sender so chunks arrive via stream_rx.
            app.engine.set_stream_sender(Some(app.stream_tx.clone()));
            app.streaming_text.clear();
            app.streaming_backend = Some(backend);
            let engine_tx2 = engine_tx.clone();
            let result = app.engine.prompt_in_cwd(backend, cwd, &prompt).await;
            app.engine.set_stream_sender(None);
            let _ = engine_tx2.send(result.map(|text| (backend, text))).await;
        }

        tokio::select! {
            _ = tick.tick() => {
                app.tick_count += 1;
            }

            // Streaming output chunks — drain all available, then redraw.
            Some(chunk) = app.stream_rx.recv(), if app.running_turn => {
                app.streaming_text.push_str(&chunk);
                // Drain any more chunks that are already buffered.
                while let Ok(c) = app.stream_rx.try_recv() {
                    app.streaming_text.push_str(&c);
                }
                // Force a redraw this iteration by resetting last_draw.
                last_draw = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_millis(MIN_FRAME_MS))
                    .unwrap_or(std::time::Instant::now());
            }

            // Incoming approval requests from the ACP layer
            Some(req) = approval_rx.recv() => {
                let tool_name = req.tool_name.clone();
                app.pending_approval = Some(req);
                app.history.push(ConversationEntry::SystemNotice {
                    text: format!("Approval requested: {}", tool_name),
                });
            }

            // Collect engine result
            Some(result) = engine_rx.recv() => {
                match result {
                    Ok((backend, text)) => {
                        app.history.push(ConversationEntry::AssistantMessage { backend, text });
                    }
                    Err(err) => {
                        app.history.push(ConversationEntry::SystemNotice {
                            text: format!("Error: {}", err),
                        });
                    }
                }
                app.running_turn = false;
                app.streaming_text.clear();
                app.streaming_backend = None;
                app.turn_started_at = None;
                app.history.scroll_to_bottom();
                // Fire queued prompt if any.
                if let Some(queued) = app.queued_prompt.take() {
                    app.history.push(ConversationEntry::UserMessage { text: queued.clone() });
                    app.history.scroll_to_bottom();
                    app.running_turn = true;
                    app.turn_started_at = Some(std::time::Instant::now());
                    let tx = app.turn_tx.clone();
                    let _ = tx.try_send(Err(anyhow::anyhow!("__prompt__:{}", queued)));
                }
            }

            // Pick up the internal "submit" signal from the channel
            Some(msg) = app.turn_rx.recv() => {
                if let Err(ref e) = msg {
                    let s = e.to_string();
                    if let Some(prompt) = s.strip_prefix("__prompt__:") {
                        pending_prompt = Some((
                            app.active_backend,
                            app.cwd.clone(),
                            prompt.to_string(),
                        ));
                    }
                }
            }

            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else { break };
                match event {
                    CEvent::Key(key) => {
                        // ── Approval overlay ────────────────────────────────
                        if app.pending_approval.is_some() {
                            match (key.modifiers, key.code) {
                                (KeyModifiers::CONTROL, KeyCode::Char('c')) => break,
                                (KeyModifiers::NONE, KeyCode::Char('y'))
                                | (KeyModifiers::NONE, KeyCode::Char('Y')) => {
                                    if let Some(req) = app.pending_approval.take() {
                                        let _ = req.reply.send(true);
                                        app.history.push(ConversationEntry::ToolResult {
                                            name: req.tool_name,
                                            ok: true,
                                            text: "approved".to_string(),
                                        });
                                    }
                                }
                                (KeyModifiers::NONE, KeyCode::Char('n'))
                                | (KeyModifiers::NONE, KeyCode::Char('N'))
                                | (KeyModifiers::NONE, KeyCode::Esc) => {
                                    if let Some(req) = app.pending_approval.take() {
                                        let _ = req.reply.send(false);
                                        app.history.push(ConversationEntry::ToolResult {
                                            name: req.tool_name,
                                            ok: false,
                                            text: "denied".to_string(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // ── Pager overlay ────────────────────────────────────
                        if let Overlay::Pager { ref mut scroll } = app.overlay {
                            match (key.modifiers, key.code) {
                                (KeyModifiers::NONE, KeyCode::Char('q'))
                                | (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                                    app.overlay = Overlay::None;
                                }
                                (KeyModifiers::NONE, KeyCode::Char('j'))
                                | (KeyModifiers::NONE, KeyCode::Down)
                                | (KeyModifiers::NONE, KeyCode::PageDown) => {
                                    *scroll = scroll.saturating_add(5);
                                }
                                (KeyModifiers::NONE, KeyCode::Char('k'))
                                | (KeyModifiers::NONE, KeyCode::Up)
                                | (KeyModifiers::NONE, KeyCode::PageUp) => {
                                    *scroll = scroll.saturating_sub(5);
                                }
                                (KeyModifiers::NONE, KeyCode::Home) => { *scroll = 0; }
                                (KeyModifiers::NONE, KeyCode::End)   => { *scroll = usize::MAX; }
                                (KeyModifiers::CONTROL, KeyCode::Char('c')) => break,
                                _ => {}
                            }
                            continue;
                        }

                        // ── Help overlay ─────────────────────────────────────
                        if app.overlay == Overlay::Help {
                            app.overlay = Overlay::None;
                            continue;
                        }

                        // ── Quit confirm ─────────────────────────────────────
                        if app.overlay == Overlay::QuitConfirm {
                            if matches!((key.modifiers, key.code),
                                (KeyModifiers::CONTROL, KeyCode::Char('c'))) {
                                break;
                            }
                            app.overlay = Overlay::None;
                            app.quit_confirm_tick = None;
                            continue;
                        }

                        // ── Global shortcuts ─────────────────────────────────
                        match (key.modifiers, key.code) {
                            // Quit with double Ctrl+C
                            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                                if app.quit_confirm_tick.is_some() {
                                    break;
                                }
                                app.quit_confirm_tick = Some(app.tick_count);
                                app.overlay = Overlay::QuitConfirm;
                            }

                            // Esc — interrupt running turn or dismiss overlay
                            (KeyModifiers::NONE, KeyCode::Esc) => {
                                if app.running_turn {
                                    // Signal cancellation (best-effort; engine will finish)
                                    app.history.push(ConversationEntry::SystemNotice {
                                        text: "Interrupted (waiting for engine to finish…)".into(),
                                    });
                                }
                                app.overlay = Overlay::None;
                            }

                            // Ctrl+T — toggle pager
                            (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                                app.overlay = Overlay::Pager { scroll: usize::MAX };
                            }

                            // ? — toggle help
                            (KeyModifiers::NONE, KeyCode::Char('?')) => {
                                app.overlay = Overlay::Help;
                            }

                            // Backend cycle
                            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                                app.cycle_backend();
                            }

                            // Clear history
                            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                                app.history.entries.clear();
                                app.history.scroll_to_bottom();
                            }

                            // Page scroll (when composer is empty / history focused)
                            (KeyModifiers::NONE, KeyCode::PageUp) => {
                                app.history.scroll_up(10);
                            }
                            (KeyModifiers::NONE, KeyCode::PageDown) => {
                                app.history.scroll_down(10);
                            }

                            // Tab — queue if running, otherwise submit
                            (KeyModifiers::NONE, KeyCode::Tab) => {
                                if app.running_turn && !app.composer.text.trim().is_empty() {
                                    app.submit(); // submit() handles queue logic
                                }
                            }

                            // All other keys → composer
                            _ => {
                                let action = app.composer.handle_key(key);
                                match action {
                                    ComposerAction::Submit => app.submit(),
                                    ComposerAction::ScrollHistory => {
                                        if key.code == KeyCode::Up {
                                            app.history.scroll_up(1);
                                        } else {
                                            app.history.scroll_down(1);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Clear quit-confirm if any key other than Ctrl+C is pressed.
                        if app.overlay != Overlay::QuitConfirm {
                            app.quit_confirm_tick = None;
                        }
                        // Auto-expire quit-confirm after ~2 seconds (25 ticks @ 80ms).
                        if let Some(t) = app.quit_confirm_tick {
                            if app.tick_count.saturating_sub(t) > 25 {
                                app.overlay = Overlay::None;
                                app.quit_confirm_tick = None;
                            }
                        }
                    }
                    CEvent::Resize(_, _) => { /* ratatui handles resize on next draw */ }
                    _ => {}
                }
            }
        }
    }

    app.engine.shutdown_all_clients().await;
    Ok(())
}
