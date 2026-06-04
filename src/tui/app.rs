use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};

use crate::config::Config;
use crate::update_check;

use super::prune::PrunePhase;
use super::{detail, help, home, projects, prune, update};

/// Which top-level screen is currently shown.
#[derive(Clone, Copy)]
pub enum Screen {
    Home,
    Projects,
    ProjectDetail,
    Update,
    Prune,
}

pub struct App {
    pub cfg: Config,
    pub screen: Screen,
    pub should_quit: bool,
    /// Monotonic frame counter, used to animate spinners.
    pub tick: u64,
    pub home: home::HomeState,
    pub projects: projects::ProjectsState,
    pub detail: Option<detail::DetailState>,
    pub update: Option<update::UpdateState>,
    pub prune: Option<prune::PruneState>,
    pub show_help: bool,
    pub update_available: Option<String>,
    update_rx: Option<Receiver<Option<String>>>,
}

impl App {
    fn new(cfg: Config) -> Self {
        Self {
            cfg,
            screen: Screen::Home,
            should_quit: false,
            tick: 0,
            home: home::HomeState::new(),
            projects: projects::ProjectsState::new(),
            detail: None,
            update: None,
            prune: None,
            show_help: false,
            update_available: None,
            update_rx: Some(update_check::spawn_check()),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // `?` toggles the help overlay from anywhere; while open it swallows
        // navigation so nothing happens behind it (q still quits).
        if key.code == KeyCode::Char('?') {
            self.show_help = !self.show_help;
            return;
        }
        if self.show_help {
            match key.code {
                KeyCode::Esc => self.show_help = false,
                KeyCode::Char('q') => self.should_quit = true,
                _ => {}
            }
            return;
        }
        match self.screen {
            Screen::Home => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                KeyCode::Up | KeyCode::Char('k') => self.home.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.home.next(),
                KeyCode::Enter => self.open_selected_feature(),
                _ => {}
            },
            Screen::Projects => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => self.screen = Screen::Home,
                KeyCode::Up | KeyCode::Char('k') => self.projects.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.projects.next(),
                KeyCode::Enter => self.open_selected_project(),
                KeyCode::Char('r') => self.projects.start_scan(&self.cfg),
                KeyCode::Char('u') => self.start_update_all(),
                KeyCode::Char('p') => self.start_prune_all(),
                _ => {}
            },
            Screen::ProjectDetail => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => {
                    self.screen = Screen::Projects;
                    self.detail = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(d) = self.detail.as_mut() {
                        d.prev();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(d) = self.detail.as_mut() {
                        d.next();
                    }
                }
                KeyCode::Char('u') => self.start_update_current(),
                KeyCode::Char('p') => self.start_prune_current(),
                _ => {}
            },
            Screen::Update => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => self.finish_update(),
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(u) = self.update.as_mut() {
                        u.prev();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(u) = self.update.as_mut() {
                        u.next();
                    }
                }
                _ => {}
            },
            Screen::Prune => self.on_prune_key(key.code),
        }
    }

    fn on_prune_key(&mut self, code: KeyCode) {
        let Some((phase, confirm)) = self.prune.as_ref().map(|p| (p.phase, p.confirm)) else {
            return;
        };
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(p) = self.prune.as_mut() {
                    p.prev();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(p) = self.prune.as_mut() {
                    p.next();
                }
            }
            KeyCode::Char(' ') if phase == PrunePhase::Review && !confirm => {
                if let Some(p) = self.prune.as_mut() {
                    p.toggle_selected();
                }
            }
            KeyCode::Char('a') if phase == PrunePhase::Review && !confirm => {
                if let Some(p) = self.prune.as_mut() {
                    p.toggle_all();
                }
            }
            KeyCode::Char('d') | KeyCode::Enter if phase == PrunePhase::Review && !confirm => {
                if let Some(p) = self.prune.as_mut() {
                    if p.selected_count() > 0 {
                        p.confirm = true;
                    }
                }
            }
            KeyCode::Char('y') | KeyCode::Enter if confirm => {
                if let Some(p) = self.prune.as_mut() {
                    p.execute();
                }
            }
            KeyCode::Char('n') if confirm => {
                if let Some(p) = self.prune.as_mut() {
                    p.confirm = false;
                }
            }
            KeyCode::Esc => {
                if confirm {
                    if let Some(p) = self.prune.as_mut() {
                        p.confirm = false;
                    }
                } else {
                    self.finish_prune();
                }
            }
            _ => {}
        }
    }

    fn open_selected_feature(&mut self) {
        if home::features()[self.home.selected].key == "projects" {
            self.screen = Screen::Projects;
            if self.projects.is_empty() {
                self.projects.start_scan(&self.cfg);
            }
        }
    }

    fn open_selected_project(&mut self) {
        if let Some(project) = self
            .projects
            .table
            .selected()
            .and_then(|i| self.projects.projects.get(i))
            .cloned()
        {
            self.detail = Some(detail::DetailState::load(project, &self.cfg));
            self.screen = Screen::ProjectDetail;
        }
    }

    fn all_targets(&self) -> Vec<(String, PathBuf)> {
        self.projects
            .projects
            .iter()
            .map(|p| (p.name.clone(), p.path.clone()))
            .collect()
    }

    fn current_target(&self) -> Option<(String, PathBuf)> {
        self.detail
            .as_ref()
            .map(|d| (d.project.name.clone(), d.project.path.clone()))
    }

    fn start_update_all(&mut self) {
        let targets = self.all_targets();
        if targets.is_empty() {
            return;
        }
        let title = format!("Updating {} projects", targets.len());
        self.update = Some(update::UpdateState::start(
            title,
            targets,
            self.cfg.default_branches.clone(),
            Screen::Projects,
        ));
        self.screen = Screen::Update;
    }

    fn start_update_current(&mut self) {
        if let Some(target) = self.current_target() {
            let title = format!("Updating {}", target.0);
            self.update = Some(update::UpdateState::start(
                title,
                vec![target],
                self.cfg.default_branches.clone(),
                Screen::ProjectDetail,
            ));
            self.screen = Screen::Update;
        }
    }

    fn start_prune_all(&mut self) {
        let targets = self.all_targets();
        if targets.is_empty() {
            return;
        }
        let title = format!("Prune merged · {} projects", targets.len());
        self.prune = Some(prune::PruneState::start(
            title,
            targets,
            self.cfg.default_branches.clone(),
            Screen::Projects,
        ));
        self.screen = Screen::Prune;
    }

    fn start_prune_current(&mut self) {
        if let Some(target) = self.current_target() {
            let title = format!("Prune merged · {}", target.0);
            self.prune = Some(prune::PruneState::start(
                title,
                vec![target],
                self.cfg.default_branches.clone(),
                Screen::ProjectDetail,
            ));
            self.screen = Screen::Prune;
        }
    }

    fn finish_update(&mut self) {
        let info = self.update.as_ref().map(|u| (u.origin, u.running));
        self.update = None;
        self.return_to(info.map(|(o, r)| (o, !r)));
    }

    fn finish_prune(&mut self) {
        let origin = self.prune.as_ref().map(|p| p.origin);
        self.prune = None;
        // Branches changed — always refresh on the way out.
        self.return_to(origin.map(|o| (o, true)));
    }

    /// Return to `origin`, refreshing its data when `refresh` is set.
    fn return_to(&mut self, target: Option<(Screen, bool)>) {
        let Some((origin, refresh)) = target else {
            self.screen = Screen::Projects;
            return;
        };
        match origin {
            Screen::ProjectDetail => {
                if refresh {
                    if let Some(d) = &self.detail {
                        let project = d.project.clone();
                        self.detail = Some(detail::DetailState::load(project, &self.cfg));
                    }
                }
                self.screen = Screen::ProjectDetail;
            }
            _ => {
                self.screen = Screen::Projects;
                if refresh {
                    self.projects.start_scan(&self.cfg);
                }
            }
        }
    }

    fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.projects.drain();
        if let Some(u) = self.update.as_mut() {
            u.drain();
        }
        if let Some(p) = self.prune.as_mut() {
            p.drain();
        }
        if let Some(rx) = self.update_rx.take() {
            match rx.try_recv() {
                Ok(result) => self.update_available = result,
                Err(TryRecvError::Empty) => self.update_rx = Some(rx),
                Err(TryRecvError::Disconnected) => {}
            }
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        match self.screen {
            Screen::Home => home::draw(f, self),
            Screen::Projects => projects::draw(f, self),
            Screen::ProjectDetail => detail::draw(f, self),
            Screen::Update => update::draw(f, self),
            Screen::Prune => prune::draw(f, self),
        }
        if self.show_help {
            help::draw_overlay(f, self.screen);
        }
    }
}

pub fn run(cfg: Config) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, cfg);
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, cfg: Config) -> Result<()> {
    let mut app = App::new(cfg);
    while !app.should_quit {
        terminal.draw(|f| app.draw(f))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }
        app.on_tick();
    }
    Ok(())
}
