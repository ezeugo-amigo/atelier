use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::TableState, Terminal};
use vigil_adapter_claude::ClaudeCodeAdapter;
use vigil_core::{AgentAdapter, Session, SessionId};

use crate::archive::Archive;

const TICK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);

pub struct App {
    pub sessions: Vec<Session>,
    pub table_state: TableState,
    pub archive: Archive,
    last_refresh: Instant,
}

impl App {
    fn new() -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            sessions: Vec::new(),
            table_state,
            archive: Archive::load(),
            last_refresh: Instant::now(),
        }
    }

    pub fn selected_id(&self) -> Option<&SessionId> {
        self.table_state
            .selected()
            .and_then(|i| self.sessions.get(i))
            .map(|s| &s.id)
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.table_state
            .selected()
            .and_then(|i| self.sessions.get(i))
    }

    fn next(&mut self) {
        if self.sessions.is_empty() { return; }
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1).min(self.sessions.len() - 1)));
    }

    fn prev(&mut self) {
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some(i.saturating_sub(1)));
    }

    /// Dismiss the selected session: add to archive, remove from list immediately.
    fn dismiss_selected(&mut self) {
        let Some(i) = self.table_state.selected() else { return };
        let Some(id) = self.sessions.get(i).map(|s| s.id.clone()) else { return };
        self.archive.dismiss(&id).ok();
        self.sessions.remove(i);
        let new_sel = i.min(self.sessions.len().saturating_sub(1));
        if self.sessions.is_empty() {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(new_sel));
        }
    }
}

pub async fn run(adapter: ClaudeCodeAdapter) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, adapter).await;

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    adapter: ClaudeCodeAdapter,
) -> Result<()> {
    let mut app = App::new();
    refresh(&mut app, &adapter).await;

    loop {
        terminal.draw(|f| crate::render::draw(f, &mut app))?;

        if event::poll(POLL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => app.next(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev(),
                    KeyCode::Char('d') => app.dismiss_selected(),
                    KeyCode::Enter => {
                        if let Some(s) = app.selected_session() {
                            let id = s.id.clone();
                            let project_dir = s.project_dir.clone();
                            attach(terminal, &id, project_dir.as_deref())?;
                            terminal.clear()?;
                        }
                    }
                    _ => {}
                }
            }
        }

        if app.last_refresh.elapsed() >= TICK {
            refresh(&mut app, &adapter).await;
        }
    }

    Ok(())
}

async fn refresh(app: &mut App, adapter: &ClaudeCodeAdapter) {
    let all_ids = adapter.discover().await.unwrap_or_default();

    // Filter out dismissed sessions before doing any I/O
    let active_ids: Vec<SessionId> = all_ids
        .into_iter()
        .filter(|id| !app.archive.is_dismissed(id))
        .collect();

    // Read all remaining sessions: history loaded once, lsof in parallel
    let sessions = adapter.read_all(&active_ids).await;

    let selected_id = app.selected_id().cloned();
    app.sessions = sessions;

    let new_sel = selected_id
        .and_then(|id| app.sessions.iter().position(|s| s.id == id))
        .unwrap_or(app.table_state.selected().unwrap_or(0))
        .min(app.sessions.len().saturating_sub(1));

    if app.sessions.is_empty() {
        app.table_state.select(None);
    } else {
        app.table_state.select(Some(new_sel));
    }

    app.last_refresh = Instant::now();
}

fn attach(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    id: &SessionId,
    project_dir: Option<&std::path::Path>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let mut cmd = std::process::Command::new("claude");
    cmd.arg("--resume").arg(&id.0);
    cmd.env_remove("CLAUDECODE");
    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }
    cmd.status().ok();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;

    Ok(())
}
