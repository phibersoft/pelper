use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::app::Screen;

/// Keybindings shown in the help overlay for the active screen.
fn bindings(screen: Screen) -> Vec<(&'static str, &'static str)> {
    match screen {
        Screen::Home => vec![
            ("↑/↓ · j/k", "move"),
            ("⏎", "open feature"),
            ("?", "toggle this help"),
            ("q", "quit"),
        ],
        Screen::Projects => vec![
            ("↑/↓ · j/k", "move"),
            ("⏎", "open detail"),
            ("u", "update all"),
            ("p", "prune all merged"),
            ("r", "rescan"),
            ("?", "toggle this help"),
            ("Esc", "home"),
            ("q", "quit"),
        ],
        Screen::ProjectDetail => vec![
            ("↑/↓ · j/k", "move"),
            ("u", "update default branch"),
            ("p", "prune merged"),
            ("?", "toggle this help"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        Screen::Update => vec![
            ("↑/↓ · j/k", "move"),
            ("?", "toggle this help"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
        Screen::Prune => vec![
            ("↑/↓ · j/k", "move"),
            ("Space", "toggle selection"),
            ("a", "toggle all"),
            ("d", "delete selected"),
            ("y / n", "confirm / cancel"),
            ("?", "toggle this help"),
            ("Esc", "back"),
            ("q", "quit"),
        ],
    }
}

/// Draw a centered keybindings popup over the current screen.
pub fn draw_overlay(f: &mut Frame, screen: Screen) {
    let items = bindings(screen);
    let area = centered(46, items.len() as u16 + 3, f.area());

    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = vec![Line::from("")];
    for (keys, desc) in &items {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {keys:<12}"),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled((*desc).to_string(), Style::default().fg(Color::Gray)),
        ]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" keybindings ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let cols = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(area);
    let rows = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(cols[0]);
    rows[0]
}
