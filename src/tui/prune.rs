use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::git::{self, time::relative, DeleteResult, PruneScanMsg};

use super::app::{App, Screen};
use super::{hint, key};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, PartialEq)]
pub enum PrunePhase {
    Collecting,
    Review,
    Done,
}

pub struct Candidate {
    pub project: String,
    pub path: PathBuf,
    pub branch: String,
    pub last_commit: Option<SystemTime>,
    pub unique_commits: usize,
    pub selected: bool,
    pub result: Option<DeleteResult>,
}

pub struct PruneState {
    pub title: String,
    pub phase: PrunePhase,
    pub candidates: Vec<Candidate>,
    pub table: TableState,
    pub origin: Screen,
    /// The second step of the delete confirmation.
    pub confirm: bool,
    rx: Option<Receiver<PruneScanMsg>>,
}

impl PruneState {
    pub fn start(
        title: String,
        targets: Vec<(String, PathBuf)>,
        default_branches: Vec<String>,
        origin: Screen,
    ) -> Self {
        let rx = git::spawn_prune_scan(targets, default_branches);
        Self {
            title,
            phase: PrunePhase::Collecting,
            candidates: Vec::new(),
            table: TableState::default(),
            origin,
            confirm: false,
            rx: Some(rx),
        }
    }

    pub fn drain(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut done = false;
        for msg in rx.try_iter() {
            match msg {
                PruneScanMsg::Found(c) => self.candidates.push(Candidate {
                    project: c.project,
                    path: c.path,
                    branch: c.branch,
                    last_commit: c.last_commit,
                    unique_commits: c.unique_commits,
                    selected: true, // gone ⇒ merged in the user's workflow; opt-out
                    result: None,
                }),
                PruneScanMsg::Done => {
                    done = true;
                    break;
                }
            }
        }
        if self.table.selected().is_none() && !self.candidates.is_empty() {
            self.table.select(Some(0));
        }
        if done {
            self.phase = PrunePhase::Review;
        } else {
            self.rx = Some(rx);
        }
    }

    pub fn next(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len();
        let i = self.table.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.table.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len();
        let i = self
            .table
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.table.select(Some(i));
    }

    pub fn toggle_selected(&mut self) {
        if let Some(c) = self.table.selected().and_then(|i| self.candidates.get_mut(i)) {
            c.selected = !c.selected;
        }
    }

    pub fn toggle_all(&mut self) {
        let all = self.candidates.iter().all(|c| c.selected);
        for c in &mut self.candidates {
            c.selected = !all;
        }
    }

    pub fn selected_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.selected).count()
    }

    /// Delete every selected candidate and move to the results view.
    pub fn execute(&mut self) {
        for c in &mut self.candidates {
            if c.selected {
                c.result = Some(git::delete_branch(&c.path, &c.branch));
            }
        }
        self.confirm = false;
        self.phase = PrunePhase::Done;
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let tick = app.tick;
    let Some(state) = app.prune.as_mut() else {
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_header(f, state, tick, chunks[0]);
    match state.phase {
        PrunePhase::Collecting => {
            super::empty_state(f, "Looking for merged (gone) branches…", Color::Yellow, chunks[1])
        }
        PrunePhase::Review if state.candidates.is_empty() => {
            super::empty_state(f, "✓ No merged branches to prune.", Color::Green, chunks[1])
        }
        PrunePhase::Review => draw_review_table(f, state, chunks[1]),
        PrunePhase::Done if state.candidates.is_empty() => {
            super::empty_state(f, "Nothing to do.", Color::DarkGray, chunks[1])
        }
        PrunePhase::Done => draw_done_table(f, state, chunks[1]),
    }
    draw_footer(f, state, chunks[2]);
}

fn draw_header(f: &mut Frame, state: &PruneState, tick: u64, area: Rect) {
    let status = match state.phase {
        PrunePhase::Collecting => {
            let frame = SPINNER[(tick as usize) % SPINNER.len()];
            Span::styled(
                format!("{frame} scanning… ({} found)", state.candidates.len()),
                Style::default().fg(Color::Yellow),
            )
        }
        PrunePhase::Review => Span::styled(
            format!("{} candidate(s) · {} selected", state.candidates.len(), state.selected_count()),
            Style::default().fg(Color::Cyan),
        ),
        PrunePhase::Done => {
            let deleted = state
                .candidates
                .iter()
                .filter(|c| matches!(&c.result, Some(r) if r.ok))
                .count();
            let failed = state
                .candidates
                .iter()
                .filter(|c| matches!(&c.result, Some(r) if !r.ok))
                .count();
            Span::styled(
                format!("deleted {deleted} · failed {failed}"),
                Style::default().fg(Color::Green),
            )
        }
    };

    let title = Line::from(vec![
        Span::styled(
            format!(" {} ", state.title),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        status,
    ]);
    let hint_line = Line::from(Span::styled(
        " gone = remote branch deleted (merged). 'N ahead' is normal for squash-merges; -D is reflog-recoverable.",
        Style::default().fg(Color::DarkGray),
    ));

    let widget = Paragraph::new(vec![title, hint_line]).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(widget, area);
}

fn draw_review_table(f: &mut Frame, state: &mut PruneState, area: Rect) {
    let rows: Vec<Row> = state.candidates.iter().map(review_row).collect();
    let widths = [
        Constraint::Length(3),
        Constraint::Length(22),
        Constraint::Min(20),
        Constraint::Length(9),
        Constraint::Length(20),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from(""),
                Cell::from("PROJECT"),
                Cell::from("BRANCH"),
                Cell::from("LAST"),
                Cell::from("STATUS"),
            ])
            .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        )
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(Color::Rgb(40, 42, 54)))
        .highlight_symbol("› ");
    f.render_stateful_widget(table, area, &mut state.table);
}

fn review_row(c: &Candidate) -> Row<'static> {
    let check = if c.selected {
        Span::styled("[x]", Style::default().fg(Color::Green))
    } else {
        Span::styled("[ ]", Style::default().fg(Color::DarkGray))
    };
    let branch_style = if c.unique_commits > 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    let status = if c.unique_commits == 0 {
        Span::styled("merged", Style::default().fg(Color::Green))
    } else {
        Span::styled(
            format!("{} ahead", c.unique_commits),
            Style::default().fg(Color::Yellow),
        )
    };
    let last = c.last_commit.map(relative).unwrap_or_else(|| "-".to_string());

    Row::new(vec![
        Cell::from(Line::from(check)),
        Cell::from(Line::from(Span::styled(
            c.project.clone(),
            Style::default().fg(Color::DarkGray),
        ))),
        Cell::from(Line::from(Span::styled(c.branch.clone(), branch_style))),
        Cell::from(Line::from(Span::styled(
            last,
            Style::default().fg(Color::DarkGray),
        ))),
        Cell::from(Line::from(status)),
    ])
}

fn draw_done_table(f: &mut Frame, state: &mut PruneState, area: Rect) {
    let rows: Vec<Row> = state.candidates.iter().map(done_row).collect();
    let widths = [
        Constraint::Length(2),
        Constraint::Length(22),
        Constraint::Length(26),
        Constraint::Min(16),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from(""),
                Cell::from("PROJECT"),
                Cell::from("BRANCH"),
                Cell::from("RESULT"),
            ])
            .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        )
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(Color::Rgb(40, 42, 54)))
        .highlight_symbol("› ");
    f.render_stateful_widget(table, area, &mut state.table);
}

fn done_row(c: &Candidate) -> Row<'static> {
    let (icon, detail, style) = match &c.result {
        Some(r) if r.ok => {
            let detail = match &r.sha {
                Some(sha) => format!("deleted (was {sha})"),
                None => "deleted".to_string(),
            };
            (
                Span::styled("✓", Style::default().fg(Color::Green)),
                detail,
                Style::default().fg(Color::Gray),
            )
        }
        Some(r) => (
            Span::styled("✗", Style::default().fg(Color::Red)),
            r.message.clone(),
            Style::default().fg(Color::Red),
        ),
        None => (
            Span::styled("–", Style::default().fg(Color::DarkGray)),
            "kept".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    };

    Row::new(vec![
        Cell::from(Line::from(icon)),
        Cell::from(Line::from(Span::styled(
            c.project.clone(),
            Style::default().fg(Color::DarkGray),
        ))),
        Cell::from(Line::from(Span::styled(
            c.branch.clone(),
            Style::default().fg(Color::White),
        ))),
        Cell::from(Line::from(Span::styled(detail, style))),
    ])
}

fn draw_footer(f: &mut Frame, state: &PruneState, area: Rect) {
    let line = match state.phase {
        PrunePhase::Collecting => {
            Line::from(vec![key("Esc"), hint(" cancel  "), key("q"), hint(" quit")])
        }
        PrunePhase::Review if state.confirm => Line::from(vec![
            Span::styled(
                format!(" Delete {} branch(es)? ", state.selected_count()),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            key("y"),
            hint(" confirm  "),
            key("n"),
            hint(" cancel"),
        ]),
        PrunePhase::Review => Line::from(vec![
            key("↑/↓"),
            hint(" move  "),
            key("Space"),
            hint(" toggle  "),
            key("a"),
            hint(" all  "),
            key("d"),
            hint(" delete  "),
            key("Esc"),
            hint(" back"),
        ]),
        PrunePhase::Done => Line::from(vec![key("Esc"), hint(" back  "), key("q"), hint(" quit")]),
    };
    f.render_widget(Paragraph::new(line), area);
}
