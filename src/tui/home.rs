use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::app::App;
use super::{hint, key};

/// A top-level capability shown on the home screen. New features are added by
/// appending an entry here and routing its `key` in `App::open_selected_feature`.
pub struct Feature {
    pub key: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
}

pub fn features() -> &'static [Feature] {
    &[Feature {
        key: "projects",
        title: "Projects",
        desc: "Scan git repos · view · update · prune",
    }]
}

pub struct HomeState {
    pub selected: usize,
    pub list: ListState,
}

impl HomeState {
    pub fn new() -> Self {
        let mut list = ListState::default();
        list.select(Some(0));
        Self { selected: 0, list }
    }

    pub fn next(&mut self) {
        let n = features().len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected + 1) % n;
        self.list.select(Some(self.selected));
    }

    pub fn prev(&mut self) {
        let n = features().len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected + n - 1) % n;
        self.list.select(Some(self.selected));
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    let title = Line::from(vec![
        Span::styled(
            "  pelper",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  ·  your terminal DX helper",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(vec![Line::from(""), title]), chunks[0]);

    let items: Vec<ListItem> = features()
        .iter()
        .map(|feat| {
            ListItem::new(vec![
                Line::from(Span::styled(
                    feat.title,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  {}", feat.desc),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" features "),
        )
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .highlight_symbol("› ");
    f.render_stateful_widget(list, chunks[1], &mut app.home.list);

    let footer = Paragraph::new(Line::from(vec![
        key("↑/↓"),
        hint(" move  "),
        key("⏎"),
        hint(" open  "),
        key("?"),
        hint(" help  "),
        key("q"),
        hint(" quit"),
    ]))
    .alignment(Alignment::Left);
    f.render_widget(footer, chunks[2]);
}
