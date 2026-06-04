mod app;
mod detail;
mod help;
mod home;
mod projects;
mod prune;
mod update;

use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::config::Config;

/// Launch the interactive TUI.
pub fn run(cfg: Config) -> Result<()> {
    app::run(cfg)
}

/// A keycap-style hint label, e.g. ` Esc `.
fn key(s: &str) -> Span<'static> {
    Span::styled(
        format!(" {s} "),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )
}

/// Dim helper text shown between keycaps.
fn hint(s: &'static str) -> Span<'static> {
    Span::styled(s, Style::default().fg(Color::DarkGray))
}

/// Truncate `s` to at most `max` characters, adding an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Render a single dim message in `area` — used for empty and loading states.
fn empty_state(f: &mut Frame, text: &str, color: Color, area: Rect) {
    let widget = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(color),
        )),
    ]);
    f.render_widget(widget, area);
}
