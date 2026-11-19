use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub enum Token {
    Accent,
    Secondary,
    Success,
    Warn,
    Error,
    Info,
    Focus,
    Faint,
    Title,
    Selection,
}

pub fn colour_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NO_COLOR").is_none())
}

pub fn style(token: Token) -> Style {
    use Token::*;
    let fg = match token {
        Accent | Focus => Color::LightBlue,
        Secondary => Color::LightMagenta,
        Success => Color::Green,
        Warn => Color::Yellow,
        Error => Color::Red,
        Info => Color::Cyan,
        Faint => Color::DarkGray,
        Title | Selection => Color::Reset,
    };
    let attributes = match token {
        Title => Modifier::BOLD,
        Faint => Modifier::DIM,
        Selection => Modifier::REVERSED,
        _ => Modifier::empty(),
    };
    let style = Style::default().add_modifier(attributes);
    if colour_enabled() { style.fg(fg) } else { style }
}

pub fn data_style(token: Token, selected: bool) -> Style {
    let style = style(token);
    if !selected {
        return style;
    }
    let style = style.remove_modifier(Modifier::REVERSED);
    if colour_enabled() { style.bg(Color::White) } else { style.add_modifier(Modifier::BOLD) }
}
