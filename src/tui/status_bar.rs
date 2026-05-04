use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::acp::AcpBackend;
use crate::tui::state::ObservabilityMeta;

use super::theme;

const KEY_HINTS: &[(&str, &str)] = &[
    ("↑↓", "scroll"),
    ("Ctrl+B", "backend"),
    ("Ctrl+T", "pager"),
    ("?", "help"),
    ("Ctrl+C", "quit"),
];

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    backend: AcpBackend,
    model: Option<&str>,
    is_searching: bool,
    search_query: Option<&str>,
    running: bool,
    has_queued: bool,
    quit_confirm: bool,
    latest_observability: Option<&ObservabilityMeta>,
    scrolled_up: bool,
) {
    let model_str = model.unwrap_or("—");
    let left_text = format!(" {} · {} ", backend, model_str);
    let left = Span::styled(left_text.clone(), theme::status_bar_style());

    // Middle context hint
    let middle: Option<Span> = if quit_confirm {
        Some(Span::styled(
            "  Press Ctrl+C again to quit",
            theme::tool_result_err_style(),
        ))
    } else if scrolled_up {
        Some(Span::styled(
            "  [Ctrl+D or End to jump to bottom]",
            theme::system_notice_style(),
        ))
    } else if is_searching {
        let q = search_query.unwrap_or("");
        Some(Span::styled(
            format!("  Ctrl+R search: {}_", q),
            theme::system_notice_style(),
        ))
    } else if has_queued {
        Some(Span::styled("  [queued]", theme::status_bar_hint_style()))
    } else if running {
        Some(Span::styled("  running…", theme::status_bar_hint_style()))
    } else if let Some(observability) = latest_observability {
        observability_status(observability)
            .map(|text| Span::styled(format!("  {}", text), theme::status_bar_hint_style()))
    } else {
        None
    };

    let hints_width: usize = KEY_HINTS.iter().map(|(k, h)| k.len() + h.len() + 5).sum();
    let middle_width = middle.as_ref().map(|s| s.content.len()).unwrap_or(0);
    let left_width = left_text.len();
    let pad = (area.width as usize).saturating_sub(left_width + middle_width + hints_width);

    let mut spans: Vec<Span> = vec![left];
    if let Some(m) = middle {
        spans.push(m);
    }
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    for (key, hint) in KEY_HINTS {
        spans.push(Span::styled(
            format!("[{}] ", key),
            theme::status_bar_hint_style(),
        ));
        spans.push(Span::styled(
            format!("{} ", hint),
            theme::status_bar_hint_style(),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn observability_status(observability: &ObservabilityMeta) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(total_ms) = observability.total_ms {
        parts.push(format!("{}ms", total_ms));
    }
    if let Some(tokens) = observability.total_tokens {
        parts.push(format!("{} tok", tokens));
    }
    if let Some(execution_id) = observability.execution_id.as_deref() {
        let short = execution_id.chars().take(8).collect::<String>();
        if !short.is_empty() {
            parts.push(format!("exec {}", short));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}
