//! Helpers for pushing conversation entries into the terminal's native
//! scrollback buffer via `Terminal::insert_before`.
//!
//! Inspired by codex's `insert_history.rs` — the chat transcript lives in the
//! terminal's own scrollback, leaving the inline viewport for the composer +
//! status bar. This gives users native scroll, copy and selection.

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::acp::AcpBackend;

use super::state::{ConversationEntry, ObservabilityMeta};
use super::{markdown, theme};

/// Push a single conversation entry into the terminal scrollback above the
/// inline viewport. A trailing blank line is added for breathing room.
pub(super) fn insert_entry<B: Backend>(
    terminal: &mut Terminal<B>,
    entry: &ConversationEntry,
) -> std::io::Result<()> {
    let mut lines = entry_to_lines(entry);
    if lines.is_empty() {
        return Ok(());
    }
    lines.push(Line::raw(""));
    insert_lines(terminal, lines)
}

/// Push arbitrary owned lines into terminal scrollback above the inline viewport.
pub(super) fn insert_lines<B: Backend>(
    terminal: &mut Terminal<B>,
    lines: Vec<Line<'static>>,
) -> std::io::Result<()> {
    let width = terminal.size()?.width.max(1);
    let para = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let height = para.line_count(width) as u16;
    if height == 0 {
        return Ok(());
    }
    terminal.insert_before(height, |buf| {
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(buf.area, buf);
    })
}

/// Render the iota logo + version banner. Inserted once at TUI startup so the
/// banner participates in normal terminal scrollback.
pub(super) fn banner_lines() -> Vec<Line<'static>> {
    let version = env!("CARGO_PKG_VERSION");
    let build_time = env!("BUILD_TIMESTAMP");
    vec![
        Line::from(Span::styled(
            format!("│ ιώτα  v{}-{} │", version, build_time),
            theme::banner_style(),
        )),
        Line::raw(""),
    ]
}

fn entry_to_lines(entry: &ConversationEntry) -> Vec<Line<'static>> {
    match entry {
        ConversationEntry::UserMessage { text } => user_lines(text),
        ConversationEntry::AssistantMessage {
            backend,
            text,
            observability,
        } => assistant_lines(*backend, text, observability.as_ref()),
        ConversationEntry::SystemNotice { text } => vec![Line::from(Span::styled(
            format!("── {} ──", text),
            theme::system_notice_style(),
        ))],
        ConversationEntry::ToolResult { name, ok, text } => {
            let (icon, style) = if *ok {
                ("✓", theme::tool_result_ok_style())
            } else {
                ("✗", theme::tool_result_err_style())
            };
            vec![Line::from(vec![
                Span::styled(format!("{} ", icon), style),
                Span::styled(name.clone(), theme::tool_call_style()),
                Span::raw(" → "),
                Span::raw(text.clone()),
            ])]
        }
    }
}

fn user_lines(text: &str) -> Vec<Line<'static>> {
    let label = Span::styled("●  ", theme::user_label_style());
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut first = true;
    for raw in text.split('\n') {
        if first {
            out.push(Line::from(vec![label.clone(), Span::raw(raw.to_string())]));
            first = false;
        } else {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::raw(raw.to_string()),
            ]));
        }
    }
    if out.is_empty() {
        out.push(Line::from(label));
    }
    out
}

fn assistant_lines(
    backend: AcpBackend,
    text: &str,
    observability: Option<&ObservabilityMeta>,
) -> Vec<Line<'static>> {
    let mut out = vec![Line::from(Span::styled(
        assistant_label(backend),
        theme::assistant_label_style(),
    ))];
    for md in markdown::render(text) {
        let mut spans: Vec<Span<'static>> = vec![Span::raw("     ")];
        spans.extend(md.spans);
        out.push(Line::from(spans));
    }
    if let Some(meta) = observability
        && let Some(line) = observability_line(meta)
    {
        out.push(Line::from(Span::styled(
            format!("     {}", line),
            theme::status_bar_hint_style(),
        )));
    }
    out
}

fn assistant_label(backend: AcpBackend) -> &'static str {
    match backend {
        AcpBackend::ClaudeCode => "■ cc",
        AcpBackend::Codex => "■ cx",
        AcpBackend::Gemini => "■ gm",
        AcpBackend::Hermes => "■ hm",
        AcpBackend::OpenCode => "■ oc",
    }
}

fn observability_line(meta: &ObservabilityMeta) -> Option<String> {
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
    if parts.is_empty() {
        return None;
    }
    let line = parts.join(" · ");
    meta.execution_id
        .as_deref()
        .map(|execution_id| execution_id.chars().take(8).collect::<String>())
        .filter(|short| !short.is_empty())
        .map_or(Some(line.clone()), |short| Some(format!("{}: {}", short, line)))
}

#[cfg(test)]
mod tests {
    use crate::acp::AcpBackend;

    use super::*;

    #[test]
    fn user_message_uses_solid_circle_prompt() {
        let lines = user_lines("hello");

        assert_eq!(lines[0].spans[0].content.as_ref(), "●  ");
    }

    #[test]
    fn banner_wraps_logo_and_version_with_separators() {
        let lines = banner_lines();
        let text = lines[0].spans[0].content.as_ref();

        assert!(text.starts_with("│ "));
        assert!(text.ends_with(" │"));
        assert_eq!(lines[0].spans[0].style, theme::banner_style());
    }

    #[test]
    fn claude_code_message_uses_solid_square_abbreviation() {
        let entry = ConversationEntry::AssistantMessage {
            backend: AcpBackend::ClaudeCode,
            text: "hello".into(),
            observability: None,
        };

        let lines = entry_to_lines(&entry);

        assert_eq!(lines[0].spans[0].content.as_ref(), "■ cc");
    }

    #[test]
    fn assistant_labels_use_solid_square_and_two_letter_abbreviations() {
        let cases = [
            (AcpBackend::ClaudeCode, "■ cc"),
            (AcpBackend::Codex, "■ cx"),
            (AcpBackend::Gemini, "■ gm"),
            (AcpBackend::Hermes, "■ hm"),
            (AcpBackend::OpenCode, "■ oc"),
        ];

        for (backend, expected) in cases {
            assert_eq!(assistant_label(backend), expected);
        }
    }

    #[test]
    fn observability_line_shortens_execution_id() {
        let line = observability_line(&ObservabilityMeta {
            execution_id: Some("2255c021-eb0c-494e-b538-25b4499a2b85".into()),
            total_ms: Some(4634),
            prompt_ms: Some(4199),
            total_tokens: Some(28624),
            ..ObservabilityMeta::default()
        })
        .unwrap();

        assert_eq!(line, "2255c021: total 4634ms · prompt 4199ms · 28624 tokens");
    }
}

/// Insert a help block describing the keyboard shortcuts.
pub(super) fn insert_help<B: Backend>(terminal: &mut Terminal<B>) -> std::io::Result<()> {
    let items: &[(&str, &str)] = &[
        ("Enter", "Send prompt"),
        ("Shift+Enter", "Insert newline"),
        ("Tab", "Queue prompt while running"),
        ("↑ / ↓", "History recall"),
        ("Ctrl+R", "Search history"),
        ("Ctrl+K / Ctrl+Y", "Kill / yank"),
        ("Ctrl+W", "Delete word backward"),
        ("Alt+B / Alt+F", "Word backward / forward"),
        ("Ctrl+E", "Export transcript to file"),
        ("Ctrl+B", "Cycle backend"),
        ("Ctrl+L", "Clear transcript view"),
        ("Esc", "Interrupt running turn"),
        ("?", "Show this help"),
        ("Ctrl+C", "Quit (press twice)"),
    ];
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        "── Keyboard Shortcuts ──",
        theme::system_notice_style(),
    ))];
    for (k, v) in items {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:18}", k), theme::tool_call_style()),
            Span::styled(v.to_string(), theme::assistant_text_style()),
        ]));
    }
    lines.push(Line::raw(""));
    insert_lines(terminal, lines)
}
