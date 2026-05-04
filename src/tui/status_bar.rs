use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::acp::AcpBackend;

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
    } else if is_searching {
        let q = search_query.unwrap_or("");
        Some(Span::styled(
            format!("  Ctrl+R search: {}_", q),
            theme::system_notice_style(),
        ))
    } else if has_queued {
        Some(Span::styled(
            "  [queued]",
            theme::status_bar_hint_style(),
        ))
    } else if running {
        Some(Span::styled(
            "  running…",
            theme::status_bar_hint_style(),
        ))
    } else {
        None
    };

    let hints_width: usize = KEY_HINTS.iter().map(|(k, h)| k.len() + h.len() + 5).sum();
    let middle_width = middle.as_ref().map(|s| s.content.len()).unwrap_or(0);
    let left_width = left_text.len();
    let pad = (area.width as usize)
        .saturating_sub(left_width + middle_width + hints_width);

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
