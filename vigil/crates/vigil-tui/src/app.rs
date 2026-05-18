use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use atelier_worktree::{CreateOptions, Registry, RemoveOptions, WorktreeEntry};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::TableState, Terminal};
use vigil_core::{AgentAdapter, AgentKind, Container, ProbeResult, SessionId};

use crate::archive::Archive;

const TICK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);

const AGENTS: [AgentKind; 4] = [
    AgentKind::ClaudeCode,
    AgentKind::Codex,
    AgentKind::Pi,
    AgentKind::OpenCode,
];

pub enum Overlay {
    None,
    NewWorktree {
        name_buf: String,
        agent_idx: usize,
        repo_root: Option<PathBuf>,
    },
    DismissConfirm {
        container_id: String,
    },
    RemoveConfirm {
        entry: WorktreeEntry,
    },
    LogView {
        container_id: String,
        worktree_path: PathBuf,
        agent: AgentKind,
        lines: Vec<String>,
    },
}

pub struct App {
    pub containers: Vec<Container>,
    pub table_state: TableState,
    pub archive: Archive,
    pub overlay: Overlay,
    pub registry: Option<Registry>,
    last_refresh: Instant,
    last_dismissed: Option<Container>,
}

impl App {
    fn new() -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            containers: Vec::new(),
            table_state,
            archive: Archive::load(),
            overlay: Overlay::None,
            registry: Registry::load().ok(),
            last_refresh: Instant::now(),
            last_dismissed: None,
        }
    }

    pub fn selected(&self) -> Option<&Container> {
        self.table_state
            .selected()
            .and_then(|i| self.containers.get(i))
    }

    pub fn overlay_is_none(&self) -> bool {
        matches!(self.overlay, Overlay::None)
    }

    /// Whether the selected container can be removed (it's registered in the registry).
    pub fn can_remove_selected(&self) -> bool {
        self.selected().is_some()
    }

    fn next(&mut self) {
        if self.containers.is_empty() { return; }
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1).min(self.containers.len() - 1)));
    }

    fn prev(&mut self) {
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some(i.saturating_sub(1)));
    }

    pub fn open_log_view(&mut self) {
        if let Some(c) = self.selected() {
            self.overlay = Overlay::LogView {
                container_id: c.id.clone(),
                worktree_path: c.worktree_path.clone(),
                agent: c.agent,
                lines: vec![],
            };
        }
    }

    fn open_dismiss_confirm(&mut self) {
        if let Some(c) = self.selected() {
            self.overlay = Overlay::DismissConfirm { container_id: c.id.clone() };
        }
    }

    fn confirm_dismiss(&mut self) {
        let id = match &self.overlay {
            Overlay::DismissConfirm { container_id } => container_id.clone(),
            _ => return,
        };
        self.overlay = Overlay::None;
        // Find the index matching this id and dismiss it
        if let Some(i) = self.containers.iter().position(|c| c.id == id) {
            let container = self.containers[i].clone();
            self.archive.dismiss_id(&container.id).ok();
            self.last_dismissed = Some(container);
            self.containers.remove(i);
            let new_sel = i.min(self.containers.len().saturating_sub(1));
            if self.containers.is_empty() {
                self.table_state.select(None);
            } else {
                self.table_state.select(Some(new_sel));
            }
        }
    }

    fn undo_dismiss(&mut self) {
        let Some(container) = self.last_dismissed.take() else { return };
        self.archive.restore_id(&container.id).ok();
        self.containers.insert(0, container);
        self.table_state.select(Some(0));
    }

    fn open_new_worktree(&mut self) {
        if self.registry.is_none() { return; }
        let repo_root = self.selected().and_then(|c| Some(c.repo_root.clone()));
        self.overlay = Overlay::NewWorktree { name_buf: String::new(), agent_idx: 0, repo_root };
    }

    fn open_remove_confirm(&mut self) {
        let entry = {
            let c = match self.selected() { Some(c) => c, None => return };
            let reg = match self.registry.as_ref() { Some(r) => r, None => return };
            match reg.find_by_id(&c.id) { Some(e) => e.clone(), None => return }
        };
        self.overlay = Overlay::RemoveConfirm { entry };
    }

    fn cycle_agent(&mut self) {
        if let Overlay::NewWorktree { ref mut agent_idx, .. } = self.overlay {
            *agent_idx = (*agent_idx + 1) % AGENTS.len();
        }
    }

    fn wt_name_push(&mut self, c: char) {
        if let Overlay::NewWorktree { ref mut name_buf, .. } = self.overlay {
            name_buf.push(c);
        }
    }

    fn wt_name_backspace(&mut self) {
        if let Overlay::NewWorktree { ref mut name_buf, .. } = self.overlay {
            name_buf.pop();
        }
    }

    /// Returns the (worktree_path, agent_kind) to launch after the overlay closes, if any.
    fn confirm_new_worktree(&mut self) -> Option<(std::path::PathBuf, AgentKind)> {
        let (name, agent_kind, repo_root) = match &self.overlay {
            Overlay::NewWorktree { name_buf, agent_idx, repo_root } => {
                let name = if name_buf.is_empty() { None } else { Some(name_buf.clone()) };
                (name, AGENTS[*agent_idx], repo_root.clone())
            }
            _ => return None,
        };
        self.overlay = Overlay::None;
        if let Some(registry) = self.registry.as_mut() {
            let opts = CreateOptions {
                name,
                agent: agent_kind,
                repo_root,
                worktree_dir: None,
                no_launch: true,
            };
            if let Ok(entry) = atelier_worktree::create(opts, registry, None) {
                return Some((entry.worktree_path, agent_kind));
            }
        }
        None
    }

    fn confirm_remove(&mut self) {
        let id = match &self.overlay {
            Overlay::RemoveConfirm { entry } => entry.id.clone(),
            _ => return,
        };
        self.overlay = Overlay::None;
        if let Some(registry) = self.registry.as_mut() {
            atelier_worktree::remove(&id, RemoveOptions { force: false }, registry).ok();
        }
    }
}


pub type AdapterMap = HashMap<AgentKind, Arc<dyn AgentAdapter>>;

pub async fn run(adapters: AdapterMap) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, adapters).await;

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    adapters: AdapterMap,
) -> Result<()> {
    let mut app = App::new();
    refresh(&mut app, &adapters).await;

    loop {
        terminal.draw(|f| crate::render::draw(f, &mut app))?;

        if event::poll(POLL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }

                if app.overlay_is_none() {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.prev(),
                        KeyCode::Char('l') => app.open_log_view(),
                        KeyCode::Char('d') => app.open_dismiss_confirm(),
                        KeyCode::Char('u') => app.undo_dismiss(),
                        KeyCode::Char('W') => app.open_new_worktree(),
                        KeyCode::Char('R') => app.open_remove_confirm(),
                        KeyCode::Enter => {
                            if let Some(c) = app.selected() {
                                let session_id = c.session_id.clone();
                                let path = c.worktree_path.clone();
                                let agent = c.agent;
                                attach_or_launch(terminal, &adapters, session_id.as_ref(), &path, agent)?;
                                terminal.clear()?;
                            }
                        }
                        _ => {}
                    }
                } else if matches!(app.overlay, Overlay::NewWorktree { .. }) {
                    match key.code {
                        KeyCode::Esc => { app.overlay = Overlay::None; }
                        KeyCode::Tab => { app.cycle_agent(); }
                        KeyCode::Backspace => { app.wt_name_backspace(); }
                        KeyCode::Enter => {
                            if let Some((path, agent)) = app.confirm_new_worktree() {
                                attach_or_launch(terminal, &adapters, None, &path, agent)?;
                                terminal.clear()?;
                            }
                        }
                        KeyCode::Char(c) => { app.wt_name_push(c); }
                        _ => {}
                    }
                } else if matches!(app.overlay, Overlay::LogView { .. }) {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('q') => {
                            app.overlay = Overlay::None;
                        }
                        _ => {}
                    }
                } else if matches!(app.overlay, Overlay::DismissConfirm { .. }) {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.overlay = Overlay::None;
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y') => { app.confirm_dismiss(); }
                        _ => {}
                    }
                } else if matches!(app.overlay, Overlay::RemoveConfirm { .. }) {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            app.overlay = Overlay::None;
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y') => { app.confirm_remove(); }
                        _ => {}
                    }
                }
            }
        }

        if app.last_refresh.elapsed() >= TICK {
            refresh(&mut app, &adapters).await;
        }
    }

    Ok(())
}

async fn refresh(app: &mut App, adapters: &AdapterMap) {
    app.registry = Registry::load().ok();

    let entries: Vec<WorktreeEntry> = match &app.registry {
        Some(reg) => reg.entries().iter()
            .filter(|e| !app.archive.is_dismissed_id(&e.id))
            .cloned()
            .collect(),
        None => vec![],
    };

    // Probe all containers in parallel using the appropriate adapter for each agent kind
    let mut handles = Vec::with_capacity(entries.len());
    for entry in &entries {
        let path = entry.worktree_path.clone();
        // Fall back to the first available adapter if the agent's adapter isn't registered
        let adapter = adapters.get(&entry.agent)
            .or_else(|| adapters.values().next())
            .map(Arc::clone);
        handles.push(tokio::spawn(async move {
            match adapter {
                Some(a) => a.probe(&path).await,
                None => ProbeResult::no_session(),
            }
        }));
    }

    let selected_id = app.selected().map(|c| c.id.clone());
    let mut containers = Vec::with_capacity(entries.len());

    for (entry, handle) in entries.iter().zip(handles) {
        let probe: ProbeResult = handle.await.unwrap_or_else(|_| ProbeResult::no_session());
        containers.push(Container {
            id: entry.id.clone(),
            worktree_path: entry.worktree_path.clone(),
            repo_root: entry.repo_root.clone(),
            agent: entry.agent,
            branch: entry.branch.clone(),
            created_at: entry.created_at,
            state: probe.state,
            session_id: probe.session_id,
            last_activity: probe.last_activity,
            last_user_message: probe.last_user_message,
        });
    }

    app.containers = containers;

    let new_sel = selected_id
        .and_then(|id| app.containers.iter().position(|c| c.id == id))
        .unwrap_or(app.table_state.selected().unwrap_or(0))
        .min(app.containers.len().saturating_sub(1));

    if app.containers.is_empty() {
        app.table_state.select(None);
    } else {
        app.table_state.select(Some(new_sel));
    }

    // Refresh log lines if the log view overlay is open
    if let Overlay::LogView { worktree_path, agent, lines, .. } = &mut app.overlay {
        if let Some(adapter) = adapters.get(agent) {
            *lines = adapter.recent_log(worktree_path).await;
        }
    }

    app.last_refresh = Instant::now();
}

/// Attach to an existing session if `session_id` is Some, otherwise launch fresh.
fn attach_or_launch(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    adapters: &AdapterMap,
    session_id: Option<&SessionId>,
    dir: &std::path::Path,
    agent: AgentKind,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Some(adapter) = adapters.get(&agent) {
        let mut cmd = if let Some(id) = session_id {
            adapter.attach_command(id, dir)
        } else {
            adapter.launch_command(dir)
        };
        cmd.status().ok();
    }

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;

    Ok(())
}
