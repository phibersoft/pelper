use std::sync::mpsc::Receiver;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::config::Config;
use crate::git::{self, time::relative, Project, ScanMsg};

use super::app::App;
use super::{hint, key};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct ProjectsState {
    pub projects: Vec<Project>,
    pub table: TableState,
    pub scanning: bool,
    rx: Option<Receiver<ScanMsg>>,
}

impl ProjectsState {
    pub fn new() -> Self {
        Self {
            projects: Vec::new(),
            table: TableState::default(),
            scanning: false,
            rx: None,
        }
    }

    /// True before the first scan has been kicked off.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty() && self.rx.is_none()
    }

    pub fn start_scan(&mut self, cfg: &Config) {
        self.projects.clear();
        self.table.select(None);
        self.scanning = true;
        self.rx = Some(git::spawn_scan(cfg.roots.clone()));
    }

    /// Pull any results that have streamed in since the last tick.
    pub fn drain(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut got = false;
        let mut done = false;
        for msg in rx.try_iter() {
            match msg {
                ScanMsg::Found(p) => {
                    self.projects.push(*p);
                    got = true;
                }
                ScanMsg::Done => {
                    done = true;
                    break;
                }
            }
        }
        if got {
            git::sort_projects(&mut self.projects);
            if self.table.selected().is_none() && !self.projects.is_empty() {
                self.table.select(Some(0));
            }
        }
        if done {
            self.scanning = false;
        } else {
            self.rx = Some(rx);
        }
    }

    pub fn next(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        let len = self.projects.len();
        let i = self.table.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.table.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.projects.is_empty() {
            return;
        }
        let len = self.projects.len();
        let i = self
            .table
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.table.select(Some(i));
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

    draw_header(f, app, chunks[0]);
    if !app.projects.scanning && app.projects.projects.is_empty() {
        let roots = app
            .cfg
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        super::empty_state(
            f,
            &format!("No git projects under {roots} — edit ~/.config/pelper/config.toml"),
            Color::DarkGray,
            chunks[1],
        );
    } else {
        draw_table(f, app, chunks[1]);
    }
    draw_footer(f, chunks[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let st = &app.projects;
    let status = if st.scanning {
        let frame = SPINNER[(app.tick as usize) % SPINNER.len()];
        Span::styled(
            format!("{frame} scanning… ({})", st.projects.len()),
            Style::default().fg(Color::Yellow),
        )
    } else {
        Span::styled(
            format!("{} project(s)", st.projects.len()),
            Style::default().fg(Color::Green),
        )
    };

    let title = Line::from(vec![
        Span::styled(
            " Projects ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        status,
    ]);

    // Show the selected project's path (or its error), else the scanned roots.
    let sub = match st.table.selected().and_then(|i| st.projects.get(i)) {
        Some(p) if p.error.is_some() => Line::from(Span::styled(
            format!(" ⚠ {}", p.error.as_deref().unwrap_or("error")),
            Style::default().fg(Color::Red),
        )),
        Some(p) => Line::from(Span::styled(
            format!(" {}", p.path.display()),
            Style::default().fg(Color::DarkGray),
        )),
        None => {
            let roots = app
                .cfg
                .roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Line::from(Span::styled(format!(" {roots}"), Style::default().fg(Color::DarkGray)))
        }
    };

    let p = Paragraph::new(vec![title, sub]).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("PROJECT"),
        Cell::from("BRANCH"),
        Cell::from("SYNC"),
        Cell::from("LAST"),
    ])
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app.projects.projects.iter().map(project_row).collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Min(16),
        Constraint::Length(24),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 42, 54))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");

    f.render_stateful_widget(table, area, &mut app.projects.table);
}

fn project_row(p: &Project) -> Row<'static> {
    let dirty = if p.dirty {
        Span::styled("●", Style::default().fg(Color::Yellow))
    } else {
        Span::raw(" ")
    };

    let branch_style = if p.error.is_some() {
        Style::default().fg(Color::Red)
    } else if p.detached {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::Green)
    };

    let last = match p.last_commit {
        Some(t) => relative(t),
        None => "-".to_string(),
    };

    Row::new(vec![
        Cell::from(Line::from(dirty)),
        Cell::from(Line::from(Span::styled(
            p.name.clone(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ))),
        Cell::from(Line::from(Span::styled(p.branch.clone(), branch_style))),
        Cell::from(sync_line(p)),
        Cell::from(Line::from(Span::styled(
            last,
            Style::default().fg(Color::DarkGray),
        ))),
    ])
}

fn sync_line(p: &Project) -> Line<'static> {
    match p.upstream {
        None => Line::from(Span::styled("—", Style::default().fg(Color::DarkGray))),
        Some((0, 0)) => Line::from(Span::styled("✓", Style::default().fg(Color::Green))),
        Some((ahead, behind)) => {
            let mut spans = Vec::new();
            if ahead > 0 {
                spans.push(Span::styled(
                    format!("↑{ahead}"),
                    Style::default().fg(Color::Green),
                ));
            }
            if ahead > 0 && behind > 0 {
                spans.push(Span::raw(" "));
            }
            if behind > 0 {
                spans.push(Span::styled(
                    format!("↓{behind}"),
                    Style::default().fg(Color::Red),
                ));
            }
            Line::from(spans)
        }
    }
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        key("↑/↓"),
        hint(" move  "),
        key("⏎"),
        hint(" open  "),
        key("u"),
        hint(" update  "),
        key("p"),
        hint(" prune  "),
        key("r"),
        hint(" rescan  "),
        key("?"),
        hint(" help  "),
        key("Esc"),
        hint(" back  "),
        key("q"),
        hint(" quit"),
    ]));
    f.render_widget(footer, area);
}
