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
    MouseEventKind,
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
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio::task::JoinHandle;

use crate::acp::permission::{ApprovalRequest, install_tui_approval_channel};
use crate::acp::{ALL_BACKENDS, AcpBackend, AcpPromptOutput};
use crate::config::{NimiaConfig, backend_config, configured_model};
use crate::engine::IotaEngine;
use crate::telemetry::metrics;
use composer::{Composer, ComposerAction};
use state::{ConversationEntry, HistoryState, ObservabilityMeta};

type Terminal = ratatui::Terminal<CrosstermBackend<Stdout>>;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_HISTORY: usize = 500;

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
    Help,
    Pager { scroll: usize },
    QuitConfirm,
    BackendSelector { selected: usize },
}

// ── TuiApp ──────────────────────────────────────────────────────────────────

struct TuiApp {
    /// Engine is wrapped in Arc<TokioMutex> so engine calls can be spawned
    /// on a separate task without blocking the TUI event loop.
    engine: Arc<TokioMutex<IotaEngine>>,
    config: NimiaConfig,
    cwd: PathBuf,
    history: HistoryState,
    composer: Composer,
    active_backend: AcpBackend,
    active_model: Option<String>,
    running_turn: bool,
    tick_count: u64,
    /// Typed channel for submitting prompts to the engine dispatch task.
    turn_tx: mpsc::Sender<TurnMessage>,
    turn_rx: mpsc::Receiver<TurnMessage>,
    /// Pending approval request from the ACP layer (shown as an overlay).
    pending_approval: Option<ApprovalRequest>,
    latest_observability: Option<ObservabilityMeta>,
    /// Streaming output channel — receives partial chunks while engine runs.
    stream_rx: mpsc::Receiver<String>,
    /// Sender side kept so we can re-install it on the engine each turn.
    stream_tx: mpsc::Sender<String>,
    /// Accumulated streamed text for the current in-progress turn.
    streaming_text: String,
    /// Monotonically incremented each time `streaming_text` is mutated.
    streaming_version: std::cell::Cell<u64>,
    /// Cached rendered markdown lines, rebuilt when version differs.
    rendered_md_lines: std::cell::RefCell<Vec<ratatui::text::Line<'static>>>,
    rendered_version: std::cell::Cell<u64>,
    /// Backend for the current in-progress streaming turn (for display label).
    streaming_backend: Option<AcpBackend>,
    /// Active overlay (help / pager / quit-confirm).
    overlay: Overlay,
    /// Currently running engine task, if a turn is active.
    turn_task: Option<JoinHandle<()>>,
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

        let engine = Arc::new(TokioMutex::new(IotaEngine::new(
            config.clone(),
            false,
            300_000, // 5 minutes timeout for TUI
        )));
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
            latest_observability: None,
            stream_rx,
            stream_tx,
            streaming_text: String::new(),
            streaming_version: std::cell::Cell::new(0),
            rendered_md_lines: std::cell::RefCell::new(Vec::new()),
            rendered_version: std::cell::Cell::new(0),
            streaming_backend: None,
            overlay: Overlay::None,
            turn_task: None,
            turn_started_at: None,
            queued_prompt: None,
            quit_confirm_tick: None,
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
        self.history
            .push(ConversationEntry::SystemNotice { text: notice });
        self.history.scroll_to_bottom();
    }

    // ── export ───────────────────────────────────────────────────────────────

    fn export_history(&mut self) -> Result<PathBuf> {
        use std::fs;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("iota_transcript_{}.txt", timestamp);
        let path = self.cwd.join(&filename);

        let mut content = String::new();
        content.push_str(&format!("iota TUI Transcript\n"));
        content.push_str(&format!(
            "Exported: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        content.push_str(&format!("Backend: {}\n", self.active_backend));
        if let Some(model) = &self.active_model {
            content.push_str(&format!("Model: {}\n", model));
        }
        content.push_str(&format!("Working Directory: {}\n", self.cwd.display()));
        content.push_str("\n");
        content.push_str(&"=".repeat(80));
        content.push_str("\n\n");

        for entry in &self.history.entries {
            match entry {
                ConversationEntry::UserMessage { text } => {
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
                    content.push_str("\n");
                    if let Some(meta) = observability {
                        if let Some(line) = observability_line(meta, None) {
                            content.push_str(&format!("[{}]\n", line));
                        }
                    }
                    content.push_str("\n");
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
        if self.running_turn {
            // Tab-queue: store for after current turn finishes
            self.queued_prompt = Some(text);
            self.record_queued_prompt_delta(1);
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
        match tx.try_send(TurnMessage::Prompt {
            backend: self.active_backend,
            cwd: self.cwd.clone(),
            text,
        }) {
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
                self.history.push(ConversationEntry::SystemNotice {
                    text: "Error: engine channel closed".into(),
                });
                self.running_turn = false;
            }
        }
    }

    fn record_queued_prompt_delta(&self, delta: i64) {
        metrics::get().prompt_queued.add(delta, &[]);
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
            Overlay::BackendSelector { selected } => {
                self.render_backend_selector(frame, area, *selected);
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
            Constraint::Length(1),         // header
            Constraint::Fill(1),           // history
            Constraint::Length(si_height), // status indicator
            Constraint::Length(3),         // composer / approval
            Constraint::Length(1),         // status bar
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
            self.latest_observability.as_ref(),
            self.history.scroll_offset > 0, // not at bottom
        );
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let cwd_str = self.cwd.display().to_string();
        let backend_str = self.active_backend.to_string();
        let model_str = self.active_model.as_deref().unwrap_or("—");
        let version = env!("CARGO_PKG_VERSION");
        let build_time = env!("BUILD_TIMESTAMP");

        // Logo and info line
        let logo = "ιώτα"; // Greek word iota
        let info = format!(
            " {} v{}-{}  {}  [{}·{}]",
            logo, version, build_time, cwd_str, backend_str, model_str
        );
        let line = Line::from(Span::styled(info, theme::header_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_history(&self, frame: &mut Frame, area: Rect) {
        let mut lines = render_entries(&self.history.entries);

        // Live streaming text while engine responds.
        if self.running_turn && !self.streaming_text.is_empty() {
            let label = self
                .streaming_backend
                .map(|b| format!("{}  ", b))
                .unwrap_or_else(|| "…  ".to_string());
            lines.push(Line::from(Span::styled(
                label,
                theme::assistant_label_style(),
            )));
            let ver = self.streaming_version.get();
            if self.rendered_version.get() != ver {
                *self.rendered_md_lines.borrow_mut() = markdown::render(&self.streaming_text);
                self.rendered_version.set(ver);
            }
            for md_line in self.rendered_md_lines.borrow().iter() {
                let indented = Line::from(
                    std::iter::once(Span::raw("     "))
                        .chain(md_line.spans.iter().cloned())
                        .collect::<Vec<_>>(),
                );
                lines.push(indented);
            }
        }

        let total = lines.len() as u16;
        let viewport_height = area.height;
        let max_scroll = total.saturating_sub(viewport_height) as usize;
        // scroll_offset == 0 means "pinned to bottom"; increasing it scrolls up.
        let offset = self.history.scroll_offset.min(max_scroll);
        let effective_offset = max_scroll.saturating_sub(offset);

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
            // Set cursor at the beginning when empty
            frame.set_cursor_position((inner.x, inner.y));
            return;
        }

        // Multi-line: split by newlines and render without cursor marker.
        let (disp_lines, cur_row, cur_col) = self.composer.display_lines();
        let para_lines: Vec<Line> = disp_lines
            .iter()
            .map(|line_text| Line::raw(line_text.clone()))
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

        // Set the real terminal cursor position
        // Calculate display width (considering wide characters like Chinese)
        use unicode_width::UnicodeWidthStr;
        let cursor_display_width = if cur_row < disp_lines.len() {
            let line = &disp_lines[cur_row];
            let chars_before_cursor: String = line.chars().take(cur_col).collect();
            UnicodeWidthStr::width(chars_before_cursor.as_str())
        } else {
            0
        };
        let cursor_x = inner.x + cursor_display_width as u16;
        let cursor_y = inner.y + (cur_row as u16).saturating_sub(scroll_row);
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    fn render_approval_overlay(&self, frame: &mut Frame, area: Rect) {
        let tool_name = self
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

fn observability_line(meta: &ObservabilityMeta, sparkline: Option<&str>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(total_ms) = meta.total_ms {
        parts.push(format!("total {}ms", total_ms));
    }
    if let Some(prompt_ms) = meta.prompt_ms {
        parts.push(format!("prompt {}ms", prompt_ms));
    }
    if let Some(tokens) = meta.total_tokens {
        parts.push(format!("{} tokens", tokens));
    } else if meta.input_tokens.is_some() || meta.output_tokens.is_some() {
        parts.push(format!(
            "{} in / {} out",
            meta.input_tokens.unwrap_or(0),
            meta.output_tokens.unwrap_or(0)
        ));
    }
    if let Some(execution_id) = meta.execution_id.as_deref() {
        parts.push(format!("exec {}", execution_id));
    }
    if let Some(sparkline) = sparkline.filter(|value| !value.is_empty()) {
        parts.push(format!("latency {}", sparkline));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Converts a slice of [`ConversationEntry`] values into styled [`Line`]s
/// ready for a [`Paragraph`] widget.  Factored out so both `render_history`
/// and `render_pager` share identical rendering logic.
///
/// `latency_history` is filled with `total_ms` values (one per assistant turn)
/// and used to render the sparkline suffix on observability lines.
fn render_entries(entries: &std::collections::VecDeque<ConversationEntry>) -> Vec<Line<'_>> {
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut latency_history: Vec<u64> = Vec::new();
    for entry in entries.iter() {
        match entry {
            ConversationEntry::UserMessage { text } => {
                lines.push(Line::from(vec![
                    Span::styled("you  ", theme::user_label_style()),
                    Span::raw(text.clone()),
                ]));
            }
            ConversationEntry::AssistantMessage {
                backend,
                text,
                observability,
            } => {
                if let Some(total_ms) = observability.as_ref().and_then(|meta| meta.total_ms) {
                    latency_history.push(total_ms);
                }
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
                if let Some(meta) = observability {
                    if let Some(line) =
                        observability_line(meta, Some(&render_sparkline_suffix(&latency_history)))
                    {
                        lines.push(Line::from(Span::styled(
                            format!("     {}", line),
                            theme::status_bar_hint_style(),
                        )));
                    }
                }
            }
            ConversationEntry::SystemNotice { text } => {
                lines.push(Line::from(Span::styled(
                    format!("── {} ──", text),
                    theme::system_notice_style(),
                )));
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
    lines
}

fn render_sparkline_suffix(values: &[u64]) -> String {
    if values.len() < 2 {
        return String::new();
    }
    let start = values.len().saturating_sub(16);
    render_sparkline(values[start..].iter().copied())
}

fn render_sparkline(values: impl Iterator<Item = u64>) -> String {
    const TICKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let values = values.collect::<Vec<_>>();
    let Some(min) = values.iter().min().copied() else {
        return String::new();
    };
    let Some(max) = values.iter().max().copied() else {
        return String::new();
    };
    if min == max {
        return std::iter::repeat('▁').take(values.len()).collect();
    }
    values
        .into_iter()
        .map(|value| {
            let scaled = ((value - min) as f64 / (max - min) as f64)
                * (TICKS.len().saturating_sub(1) as f64);
            TICKS[scaled.round() as usize]
        })
        .collect()
}

fn observability_from_output(output: &AcpPromptOutput) -> ObservabilityMeta {
    let token_usage = output.events.iter().rev().find_map(|event| match event {
        crate::runtime_event::RuntimeEvent::TokenUsage(usage) => Some(usage),
        _ => None,
    });
    ObservabilityMeta {
        execution_id: output.execution_id.clone(),
        total_ms: Some(output.timing.total_ms),
        prompt_ms: Some(output.timing.prompt_ms),
        input_tokens: token_usage.and_then(|usage| usage.input_tokens),
        output_tokens: token_usage.and_then(|usage| usage.output_tokens),
        total_tokens: token_usage.and_then(|usage| usage.total_tokens),
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

        let lines = render_entries(&self.history.entries);

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
            .title(Span::styled(
                " Keyboard Shortcuts ",
                theme::status_bar_style(),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: &[(&str, &str)] = &[
            ("Enter", "Send prompt"),
            ("Shift+Enter", "Insert newline"),
            ("Tab", "Queue prompt while running"),
            ("↑ / ↓", "History recall"),
            ("Ctrl+R", "Search history"),
            ("Ctrl+K", "Kill to end of line"),
            ("Ctrl+Y", "Yank kill buffer"),
            ("Ctrl+W", "Delete word backward"),
            ("Alt+B / Alt+F", "Word backward / forward"),
            ("Ctrl+T", "Toggle transcript pager"),
            ("Ctrl+E", "Export history to file"),
            ("Ctrl+B", "Select backend"),
            ("Ctrl+L", "Clear history"),
            ("Ctrl+D / End", "Jump to bottom"),
            ("PageUp / PageDown", "Scroll history"),
            ("Esc", "Interrupt running turn"),
            ("? ", "Toggle this help"),
            ("Ctrl+C", "Quit (press twice)"),
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

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    /// Backend selector overlay (Ctrl+B when held or double-tap).
    fn render_backend_selector(&self, frame: &mut Frame, area: Rect, selected: usize) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::composer_border_style(true))
            .title(Span::styled(" Select Backend ", theme::status_bar_style()));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let enabled = self.enabled_backends();
        let lines: Vec<Line> = enabled
            .iter()
            .enumerate()
            .map(|(i, &backend)| {
                let model = backend_config(&self.config, backend)
                    .and_then(configured_model)
                    .unwrap_or_else(|| "—".to_string());
                let prefix = if i == selected { "▶ " } else { "  " };
                let text = format!("{}{} · {}", prefix, backend, model);
                let style = if i == selected {
                    theme::tool_call_style()
                } else {
                    theme::assistant_text_style()
                };
                Line::from(Span::styled(text, style))
            })
            .collect();

        let hint = Line::from(vec![
            Span::raw("  "),
            Span::styled("↑/↓", theme::tool_call_style()),
            Span::raw(" navigate  "),
            Span::styled("Enter", theme::tool_call_style()),
            Span::raw(" select  "),
            Span::styled("Esc", theme::tool_call_style()),
            Span::raw(" cancel"),
        ]);

        let mut all_lines = lines;
        all_lines.push(Line::raw(""));
        all_lines.push(hint);

        frame.render_widget(Paragraph::new(all_lines).wrap(Wrap { trim: false }), inner);
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
    install_tui_approval_channel(approval_tx).await;

    // Install a panic hook that restores the terminal before printing the
    // panic message, so the user's shell is not left in raw mode.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(info);
    }));

    // Terminal setup — mouse capture for scroll wheel; Option+drag to select text in macOS
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

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
    let (engine_tx, mut engine_rx) = mpsc::channel::<Result<(AcpBackend, AcpPromptOutput)>>(1);

    // pending_prompt is set by the submit path.
    let mut pending_prompt: Option<(AcpBackend, PathBuf, String)> = None;

    loop {
        let now = std::time::Instant::now();
        if now.duration_since(last_draw).as_millis() as u64 >= MIN_FRAME_MS {
            terminal.draw(|f| app.render(f))?;
            last_draw = now;
        }

        // Kick off the engine call as a non-blocking spawned task so the TUI
        // event loop (draw + input) remains responsive during engine execution.
        if let Some((backend, cwd, prompt)) = pending_prompt.take() {
            app.streaming_text.clear();
            app.streaming_version.set(app.streaming_version.get().wrapping_add(1));
            app.streaming_backend = Some(backend);
            let engine_arc = app.engine.clone();
            let stream_tx = app.stream_tx.clone();
            let engine_tx2 = engine_tx.clone();
            app.turn_task = Some(tokio::spawn(async move {
                // Lock the engine for the duration of this call.
                let mut engine = engine_arc.lock().await;
                engine.set_stream_sender(Some(stream_tx));
                let result = engine.prompt_in_cwd_timed(backend, cwd, &prompt).await;
                engine.set_stream_sender(None);
                let _ = engine_tx2
                    .send(result.map(|output| (backend, output)))
                    .await;
            }));
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
                app.streaming_version.set(app.streaming_version.get().wrapping_add(1));
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
                app.turn_task = None;
                match result {
                    Ok((backend, output)) => {
                        let observability = observability_from_output(&output);
                        app.latest_observability = Some(observability.clone());
                        app.history.push(ConversationEntry::AssistantMessage {
                            backend,
                            text: output.text,
                            observability: Some(observability),
                        });
                    }
                    Err(err) => {
                        app.history.push(ConversationEntry::SystemNotice {
                            text: format!("Error: {}", err),
                        });
                    }
                }
                app.running_turn = false;
                app.streaming_text.clear();
                app.streaming_version.set(app.streaming_version.get().wrapping_add(1));
                app.streaming_backend = None;
                app.turn_started_at = None;
                app.history.scroll_to_bottom();
                // Fire queued prompt if any.
                if let Some(queued) = app.queued_prompt.take() {
                    app.record_queued_prompt_delta(-1);
                    app.history.push(ConversationEntry::UserMessage { text: queued.clone() });
                    app.history.scroll_to_bottom();
                    app.running_turn = true;
                    app.turn_started_at = Some(std::time::Instant::now());
                    let tx = app.turn_tx.clone();
                    let _ = tx.try_send(TurnMessage::Prompt {
                        backend: app.active_backend,
                        cwd: app.cwd.clone(),
                        text: queued,
                    });
                }
            }

            // Pick up the internal "submit" signal from the channel
            Some(msg) = app.turn_rx.recv() => {
                match msg {
                    TurnMessage::Prompt { backend, cwd, text } => {
                        pending_prompt = Some((backend, cwd, text));
                    }
                }
            }

            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else { break };
                match event {
                    // ── Mouse: scroll wheel only (no click capture)
                    CEvent::Mouse(m) => {
                        match m.kind {
                            MouseEventKind::ScrollUp   => app.history.scroll_up(3),
                            MouseEventKind::ScrollDown => app.history.scroll_down(3),
                            _ => {}
                        }
                    }
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
                                | (KeyModifiers::NONE, KeyCode::Esc)
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

                        // ── Backend selector overlay ─────────────────────────
                        if let Overlay::BackendSelector { selected } = app.overlay {
                            let enabled = app.enabled_backends();
                            let mut new_selected = selected;
                            let mut should_close = false;
                            let mut should_switch = None;

                            match (key.modifiers, key.code) {
                                (KeyModifiers::NONE, KeyCode::Esc)
                                | (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                                    should_close = true;
                                }
                                (KeyModifiers::NONE, KeyCode::Up) => {
                                    new_selected = new_selected.saturating_sub(1);
                                }
                                (KeyModifiers::NONE, KeyCode::Down) => {
                                    new_selected = (new_selected + 1).min(enabled.len().saturating_sub(1));
                                }
                                (KeyModifiers::NONE, KeyCode::Enter) => {
                                    if let Some(&backend) = enabled.get(new_selected) {
                                        should_switch = Some(backend);
                                    }
                                    should_close = true;
                                }
                                (KeyModifiers::CONTROL, KeyCode::Char('c')) => break,
                                _ => {}
                            }

                            if should_close {
                                app.overlay = Overlay::None;
                            } else {
                                app.overlay = Overlay::BackendSelector { selected: new_selected };
                            }

                            if let Some(backend) = should_switch {
                                app.switch_backend(backend);
                            }

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
                                    if let Some(handle) = app.turn_task.take() {
                                        handle.abort();
                                    }
                                    app.engine.lock().await.shutdown_all_clients().await;
                                    app.running_turn = false;
                                    app.streaming_text.clear();
                                    app.streaming_version.set(app.streaming_version.get().wrapping_add(1));
                                    app.streaming_backend = None;
                                    app.turn_started_at = None;
                                    app.history.push(ConversationEntry::SystemNotice {
                                        text: "Interrupted".into(),
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

                            // Backend selector
                            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                                let enabled = app.enabled_backends();
                                if enabled.len() > 1 {
                                    let selected = enabled
                                        .iter()
                                        .position(|&b| b == app.active_backend)
                                        .unwrap_or(0);
                                    app.overlay = Overlay::BackendSelector { selected };
                                }
                            }

                            // Clear history
                            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                                app.history.entries.clear();
                                app.history.scroll_to_bottom();
                            }

                            // Export history
                            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                                match app.export_history() {
                                    Ok(path) => {
                                        app.history.push(ConversationEntry::SystemNotice {
                                            text: format!("Exported to {}", path.display()),
                                        });
                                    }
                                    Err(err) => {
                                        app.history.push(ConversationEntry::SystemNotice {
                                            text: format!("Export failed: {}", err),
                                        });
                                    }
                                }
                            }

                            // Page scroll (when composer is empty / history focused)
                            (KeyModifiers::NONE, KeyCode::PageUp) => {
                                app.history.scroll_up(10);
                            }
                            (KeyModifiers::NONE, KeyCode::PageDown) => {
                                app.history.scroll_down(10);
                            }

                            // Jump to bottom
                            (KeyModifiers::CONTROL, KeyCode::Char('d'))
                            | (KeyModifiers::NONE, KeyCode::End) => {
                                app.history.scroll_to_bottom();
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

    if let Some(handle) = app.turn_task.take() {
        handle.abort();
    }
    app.engine.lock().await.shutdown_all_clients().await;
    Ok(())
}
