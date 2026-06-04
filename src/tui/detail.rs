use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::config::Config;
use crate::git::{self, time::relative, Branch, Project};

use super::app::App;
use super::{hint, key, truncate};

/// State for the unified project-detail panel: the branch list (the "Viewer"),
/// which later hosts the update and prune actions.
pub struct DetailState {
    pub project: Project,
    pub branches: Vec<Branch>,
    pub table: TableState,
    default_branches: Vec<String>,
}

impl DetailState {
    pub fn load(project: Project, cfg: &Config) -> Self {
        let branches = git::load_branches(&project.path, &cfg.default_branches);
        let mut table = TableState::default();
        if !branches.is_empty() {
            table.select(Some(0));
        }
        Self {
            project,
            branches,
            table,
            default_branches: cfg.default_branches.clone(),
        }
    }

    pub fn next(&mut self) {
        if self.branches.is_empty() {
            return;
        }
        let len = self.branches.len();
        let i = self.table.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.table.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.branches.is_empty() {
            return;
        }
        let len = self.branches.len();
        let i = self
            .table
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.table.select(Some(i));
    }

    fn selected(&self) -> Option<&Branch> {
        self.table.selected().and_then(|i| self.branches.get(i))
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let Some(detail) = app.detail.as_mut() else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, detail, chunks[0]);
    if detail.branches.is_empty() {
        super::empty_state(f, "No local branches.", Color::DarkGray, chunks[1]);
    } else {
        draw_table(f, detail, chunks[1]);
    }
    draw_footer(f, chunks[2]);
}

fn draw_header(f: &mut Frame, detail: &DetailState, area: Rect) {
    let p = &detail.project;

    let mut title = vec![
        Span::styled(
            format!(" {} ", p.name),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  on ", Style::default().fg(Color::DarkGray)),
        Span::styled(p.branch.clone(), Style::default().fg(Color::Green)),
    ];
    if p.dirty {
        title.push(Span::styled("  ● dirty", Style::default().fg(Color::Yellow)));
    }
    title.push(Span::styled(
        format!("  ·  {} branches", detail.branches.len()),
        Style::default().fg(Color::DarkGray),
    ));

    let path = Line::from(Span::styled(
        format!(" {}", p.path.display()),
        Style::default().fg(Color::DarkGray),
    ));

    // For the highlighted branch, show its author and commit subject.
    let selected = match detail.selected() {
        Some(b) if !b.subject.is_empty() => Line::from(vec![
            Span::styled(format!(" {}", b.author), Style::default().fg(Color::Blue)),
            Span::raw("  "),
            Span::styled(
                truncate(&b.subject, area.width.saturating_sub(2) as usize),
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]),
        _ => Line::from(""),
    };

    let widget = Paragraph::new(vec![Line::from(title), path, selected]).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(widget, area);
}

fn draw_table(f: &mut Frame, detail: &mut DetailState, area: Rect) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("BRANCH"),
        Cell::from("LAST"),
        Cell::from("AUTHOR"),
        Cell::from("SYNC"),
    ])
    .style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let defaults = &detail.default_branches;
    let rows: Vec<Row> = detail
        .branches
        .iter()
        .map(|b| branch_row(b, defaults))
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Min(20),
        Constraint::Length(9),
        Constraint::Length(18),
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

    f.render_stateful_widget(table, area, &mut detail.table);
}

fn branch_row(b: &Branch, defaults: &[String]) -> Row<'static> {
    let is_primary = defaults.iter().any(|d| d == &b.name);

    let marker = if b.is_head {
        Span::styled("●", Style::default().fg(Color::Green))
    } else {
        Span::raw(" ")
    };

    let mut name_style = if b.gone {
        Style::default().fg(Color::Red)
    } else if is_primary {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };
    if b.is_head {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let last = b.last_commit.map(relative).unwrap_or_else(|| "-".to_string());

    Row::new(vec![
        Cell::from(Line::from(marker)),
        Cell::from(Line::from(Span::styled(b.name.clone(), name_style))),
        Cell::from(Line::from(Span::styled(
            last,
            Style::default().fg(Color::DarkGray),
        ))),
        Cell::from(Line::from(Span::styled(
            truncate(&b.author, 18),
            Style::default().fg(Color::DarkGray),
        ))),
        Cell::from(branch_sync(b)),
    ])
}

fn branch_sync(b: &Branch) -> Line<'static> {
    if b.gone {
        return Line::from(Span::styled(
            "gone",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if b.upstream.is_none() {
        return Line::from(Span::styled("local", Style::default().fg(Color::DarkGray)));
    }
    if b.ahead == 0 && b.behind == 0 {
        return Line::from(Span::styled("✓", Style::default().fg(Color::Green)));
    }
    let mut spans = Vec::new();
    if b.ahead > 0 {
        spans.push(Span::styled(
            format!("↑{}", b.ahead),
            Style::default().fg(Color::Green),
        ));
    }
    if b.ahead > 0 && b.behind > 0 {
        spans.push(Span::raw(" "));
    }
    if b.behind > 0 {
        spans.push(Span::styled(
            format!("↓{}", b.behind),
            Style::default().fg(Color::Red),
        ));
    }
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        key("↑/↓"),
        hint(" move  "),
        key("u"),
        hint(" update  "),
        key("p"),
        hint(" prune  "),
        key("Esc"),
        hint(" back  "),
        key("q"),
        hint(" quit"),
    ]));
    f.render_widget(footer, area);
}
