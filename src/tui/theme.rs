use ratatui::style::{Color, Modifier, Style};

pub const MAGENTA: Color = Color::Magenta;
pub const MAGENTA_LIGHT: Color = Color::LightMagenta;
pub const FG_DIM: Color = Color::DarkGray;
pub const FG_NORMAL: Color = Color::Reset;

pub fn header_style() -> Style {
    Style::default()
        .bg(MAGENTA)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub fn status_bar_style() -> Style {
    Style::default().fg(MAGENTA_LIGHT)
}

pub fn status_bar_hint_style() -> Style {
    Style::default().fg(FG_DIM)
}

pub fn user_label_style() -> Style {
    Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD)
}

pub fn assistant_label_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn assistant_text_style() -> Style {
    Style::default().fg(FG_NORMAL)
}

pub fn tool_call_style() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn tool_result_ok_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn tool_result_err_style() -> Style {
    Style::default().fg(Color::Red)
}

pub fn system_notice_style() -> Style {
    Style::default().fg(Color::Yellow)
}

pub fn composer_border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(MAGENTA)
    } else {
        Style::default().fg(FG_DIM)
    }
}

pub fn spinner_style() -> Style {
    Style::default()
        .fg(MAGENTA_LIGHT)
        .add_modifier(Modifier::BOLD)
}
