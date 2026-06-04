use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::git::{self, UpdateMsg, UpdateOutcome, UpdateStatus};

use super::app::{App, Screen};
use super::{hint, key};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub enum RowState {
    Pending,
    Running,
    Done(UpdateOutcome),
}

pub struct UpdateRow {
    pub name: String,
    pub state: RowState,
}

pub struct UpdateState {
    pub title: String,
    pub rows: Vec<UpdateRow>,
    pub table: TableState,
    /// Screen to return to when the user leaves this view.
    pub origin: Screen,
    pub running: bool,
    rx: Option<Receiver<UpdateMsg>>,
}

impl UpdateState {
    pub fn start(
        title: String,
        targets: Vec<(String, PathBuf)>,
        default_branches: Vec<String>,
        origin: Screen,
    ) -> Self {
        let rows = targets
            .iter()
            .map(|(name, _)| UpdateRow {
                name: name.clone(),
                state: RowState::Pending,
            })
            .collect();
        let rx = git::spawn_update(targets, default_branches);
        let mut table = TableState::default();
        table.select(Some(0));
        Self {
            title,
            rows,
            table,
            origin,
            running: true,
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
                UpdateMsg::Started(name) => self.set_state(&name, RowState::Running),
                UpdateMsg::Done(outcome) => {
                    let name = outcome.name.clone();
                    self.set_state(&name, RowState::Done(*outcome));
                }
                UpdateMsg::AllDone => {
                    done = true;
                    break;
                }
            }
        }
        if done {
            self.running = false;
        } else {
            self.rx = Some(rx);
        }
    }

    fn set_state(&mut self, name: &str, state: RowState) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.name == name) {
            row.state = state;
        }
    }

    pub fn next(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len();
        let i = self.table.selected().map(|i| (i + 1) % len).unwrap_or(0);
        self.table.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len();
        let i = self
            .table
            .selected()
            .map(|i| (i + len - 1) % len)
            .unwrap_or(0);
        self.table.select(Some(i));
    }

    fn done_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r.state, RowState::Done(_)))
            .count()
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let tick = app.tick;
    let Some(state) = app.update.as_mut() else {
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
    draw_table(f, state, tick, chunks[1]);
    draw_footer(f, state.running, chunks[2]);
}

fn draw_header(f: &mut Frame, state: &UpdateState, tick: u64, area: Rect) {
    let progress = if state.running {
        let frame = SPINNER[(tick as usize) % SPINNER.len()];
        Span::styled(
            format!("{frame} {}/{}", state.done_count(), state.rows.len()),
            Style::default().fg(Color::Yellow),
        )
    } else {
        Span::styled(
            format!("done · {}/{}", state.done_count(), state.rows.len()),
            Style::default().fg(Color::Green),
        )
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
        progress,
    ]);

    let widget = Paragraph::new(vec![title, summary_line(state)]).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(widget, area);
}

fn summary_line(state: &UpdateState) -> Line<'static> {
    let (mut updated, mut current, mut warning, mut failed, mut skipped) = (0, 0, 0, 0, 0);
    for row in &state.rows {
        if let RowState::Done(o) = &row.state {
            match o.status {
                UpdateStatus::Updated => updated += 1,
                UpdateStatus::UpToDate => current += 1,
                UpdateStatus::Warning => warning += 1,
                UpdateStatus::Failed => failed += 1,
                UpdateStatus::Skipped => skipped += 1,
            }
        }
    }
    Line::from(vec![
        Span::styled(format!(" ✓ {updated} updated"), Style::default().fg(Color::Green)),
        Span::styled(format!("   = {current} current"), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("   ⚠ {warning}"), Style::default().fg(Color::Yellow)),
        Span::styled(format!("   ✗ {failed}"), Style::default().fg(Color::Red)),
        Span::styled(format!("   ⊘ {skipped}"), Style::default().fg(Color::Blue)),
    ])
}

fn draw_table(f: &mut Frame, state: &mut UpdateState, tick: u64, area: Rect) {
    let rows: Vec<Row> = state.rows.iter().map(|r| update_row(r, tick)).collect();
    let widths = [
        Constraint::Length(2),
        Constraint::Length(28),
        Constraint::Min(20),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![Cell::from(""), Cell::from("PROJECT"), Cell::from("RESULT")]).style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(Color::Rgb(40, 42, 54)))
        .highlight_symbol("› ");
    f.render_stateful_widget(table, area, &mut state.table);
}

fn update_row(row: &UpdateRow, tick: u64) -> Row<'static> {
    let (icon, message, msg_style) = match &row.state {
        RowState::Pending => (
            Span::styled("·", Style::default().fg(Color::DarkGray)),
            "queued".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        RowState::Running => (
            Span::styled(
                SPINNER[(tick as usize) % SPINNER.len()],
                Style::default().fg(Color::Yellow),
            ),
            "updating…".to_string(),
            Style::default().fg(Color::Yellow),
        ),
        RowState::Done(o) => {
            let (sym, color) = match o.status {
                UpdateStatus::Updated => ("✓", Color::Green),
                UpdateStatus::UpToDate => ("=", Color::DarkGray),
                UpdateStatus::Warning => ("⚠", Color::Yellow),
                UpdateStatus::Failed => ("✗", Color::Red),
                UpdateStatus::Skipped => ("⊘", Color::Blue),
            };
            (
                Span::styled(sym, Style::default().fg(color)),
                o.message.clone(),
                Style::default().fg(Color::Gray),
            )
        }
    };

    Row::new(vec![
        Cell::from(Line::from(icon)),
        Cell::from(Line::from(Span::styled(
            row.name.clone(),
            Style::default().fg(Color::White),
        ))),
        Cell::from(Line::from(Span::styled(message, msg_style))),
    ])
}

fn draw_footer(f: &mut Frame, running: bool, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        key("↑/↓"),
        hint(" move  "),
        key("Esc"),
        hint(if running { " leave  " } else { " back  " }),
        key("q"),
        hint(" quit"),
    ]));
    f.render_widget(footer, area);
}
