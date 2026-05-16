use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::config::{backend_config, configured_model};

use super::state::{ConversationEntry, ObservabilityMeta};
use super::{SPINNER_FRAMES, TuiApp, markdown, status_bar, theme};

impl TuiApp {
    pub(super) fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        match &self.overlay {
            super::Overlay::Pager { scroll } => {
                self.render_pager(frame, area, *scroll);
                return;
            }
            super::Overlay::Help => {
                self.render_help(frame, area);
                return;
            }
            super::Overlay::BackendSelector { selected } => {
                self.render_backend_selector(frame, area, *selected);
                return;
            }
            super::Overlay::QuitConfirm => {}
            super::Overlay::None => {}
        }

        let si_height = if self.running_turn { 1 } else { 0 };

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(si_height),
            Constraint::Length(3),
            Constraint::Length(1),
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
            matches!(self.overlay, super::Overlay::QuitConfirm),
            self.latest_observability.as_ref(),
            self.history.scroll_offset > 0,
        );
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let cwd_str = self.cwd.display().to_string();
        let backend_str = self.active_backend.to_string();
        let model_str = self.active_model.as_deref().unwrap_or("—");
        let version = env!("CARGO_PKG_VERSION");
        let build_time = env!("BUILD_TIMESTAMP");

        let logo = "ιώτα";
        let info = format!(
            " {} v{}-{}  {}  [{}·{}]",
            logo, version, build_time, cwd_str, backend_str, model_str
        );
        let line = Line::from(Span::styled(info, theme::header_style()));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_history(&self, frame: &mut Frame, area: Rect) {
        let mut lines = render_entries(&self.history.entries);

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
            frame.set_cursor_position((inner.x, inner.y));
            return;
        }

        let (disp_lines, cur_row, cur_col) = self.composer.display_lines();
        let para_lines: Vec<Line> = disp_lines
            .iter()
            .map(|line_text| Line::raw(line_text.clone()))
            .collect();

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

pub(super) fn observability_line(
    meta: &ObservabilityMeta,
    sparkline: Option<&str>,
) -> Option<String> {
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
                if let Some(meta) = observability
                    && let Some(line) =
                        observability_line(meta, Some(&render_sparkline_suffix(&latency_history)))
                {
                    lines.push(Line::from(Span::styled(
                        format!("     {}", line),
                        theme::status_bar_hint_style(),
                    )));
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
        return std::iter::repeat_n('▁', values.len()).collect();
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
