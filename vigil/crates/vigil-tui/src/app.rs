use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use atelier_worktree::{CreateOptions, Registry, RemoveOptions, WorktreeEntry};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::TableState, Terminal};
use serde::{Deserialize, Serialize};
use vigil_core::{AgentAdapter, AgentKind, Container, LogEvent, PrStatus, ProbeResult, SessionId};

use crate::archive::Archive;

const TICK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);
const PR_REFRESH: Duration = Duration::from_secs(60);

#[derive(Debug, Serialize, Deserialize)]
struct RepoScanCache {
    search_dirs: Vec<PathBuf>,
    repos: Vec<PathBuf>,
}

pub const AGENTS: [AgentKind; 2] = [AgentKind::ClaudeCode, AgentKind::Pi];

fn vigil_worktrees_prefix() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        "/.vigil/worktrees/".to_string()
    } else {
        format!("{home}/.vigil/worktrees/")
    }
}

fn container_repo_group(c: &Container, vigil_wt: &str) -> String {
    let full = c.worktree_path.display().to_string();
    if let Some(rest) = full.strip_prefix(vigil_wt) {
        rest.split('/').next().unwrap_or("?").to_string()
    } else {
        c.repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string()
    }
}

pub enum MessageInputChunk {
    Text(String),
    Paste(String),
}

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
        /// Structured turn events (Pi and others with JSONL sessions).
        events: Vec<LogEvent>,
        /// Raw log lines fallback (Claude Code debug logs).
        lines: Vec<String>,
        /// Number of rendered lines scrolled up from the live bottom of the log.
        scroll: usize,
    },
    SendMessage {
        input: Vec<MessageInputChunk>,
        /// Container receiving the message. Captured when the composer opens so
        /// it also works from LogView without relying on selection changes.
        container_id: Option<String>,
        /// Return to the log view after sending/canceling when the composer was
        /// opened from LogView.
        return_to_log: bool,
    },
    /// Ctrl+P style project picker: search home directory for git repos.
    ProjectPicker {
        query: String,
        all_repos: Vec<PathBuf>,
        selected_idx: usize,
        scanning: bool,
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
    /// Cache of PR status per container id, with the time it was last fetched.
    pr_cache: HashMap<String, (Option<PrStatus>, Instant)>,
    /// Cache of log data per container id, updated every tick for the selected container.
    log_cache: HashMap<String, (Vec<vigil_core::LogEvent>, Vec<String>)>,
    /// Receiver for async git-repo scan results (feeds ProjectPicker).
    scan_rx: Option<tokio::sync::oneshot::Receiver<Vec<PathBuf>>>,
    /// Receiver for the background refresh job. The UI keeps rendering cached
    /// containers while this probes the registry/adapters/PR state off-thread.
    refresh_rx: Option<tokio::sync::oneshot::Receiver<RefreshResult>>,
    /// Last message sent via the SendMessage overlay, keyed by container id.
    /// Persists across refresh() cycles since history.jsonl isn't updated by --print mode.
    last_sent: HashMap<String, String>,
    /// After creating a new worktree, hold its path here so the next refresh
    /// can auto-select it once the background probe delivers the container.
    pending_select_path: Option<PathBuf>,
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
            log_cache: HashMap::new(),
            pr_cache: HashMap::new(),
            scan_rx: None,
            refresh_rx: None,
            last_sent: HashMap::new(),
            pending_select_path: None,
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

    pub fn cached_log_data(&self, container_id: &str) -> Option<(&[LogEvent], &[String])> {
        self.log_cache
            .get(container_id)
            .map(|(events, lines)| (events.as_slice(), lines.as_slice()))
    }

    /// Whether the selected container can be removed (it's registered in the registry).
    pub fn can_remove_selected(&self) -> bool {
        self.selected().is_some()
    }

    fn next(&mut self) {
        if self.containers.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state
            .select(Some((i + 1).min(self.containers.len() - 1)));
    }

    fn prev(&mut self) {
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some(i.saturating_sub(1)));
    }

    pub fn cycle_selected_agent(&mut self) {
        let Some(c) = self.selected() else { return };
        let id = c.id.clone();
        let current = c.agent;
        let idx = AGENTS.iter().position(|a| *a == current).unwrap_or(0);
        let next = AGENTS[(idx + 1) % AGENTS.len()];
        if let Some(registry) = self.registry.as_mut() {
            registry.update_agent(&id, next).ok();
        }
    }

    pub fn open_log_view(&mut self) {
        if let Some(c) = self.selected() {
            let (events, lines) = self.log_cache.get(&c.id).cloned().unwrap_or_default();
            self.overlay = Overlay::LogView {
                container_id: c.id.clone(),
                events,
                lines,
                scroll: 0,
            };
        }
    }

    fn open_dismiss_confirm(&mut self) {
        if let Some(c) = self.selected() {
            self.overlay = Overlay::DismissConfirm {
                container_id: c.id.clone(),
            };
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
        let Some(container) = self.last_dismissed.take() else {
            return;
        };
        self.archive.restore_id(&container.id).ok();
        self.containers.insert(0, container);
        self.table_state.select(Some(0));
    }

    pub fn open_project_picker(&mut self) {
        if self.registry.is_none() {
            return;
        }
        self.overlay = Overlay::ProjectPicker {
            query: String::new(),
            all_repos: load_repo_scan_cache(),
            selected_idx: 0,
            scanning: true,
        };
    }

    fn open_new_worktree(&mut self) {
        if self.registry.is_none() {
            return;
        }
        let repo_root = self.selected().map(|c| c.repo_root.clone());
        self.overlay = Overlay::NewWorktree {
            name_buf: String::new(),
            agent_idx: 0,
            repo_root,
        };
    }

    fn open_remove_confirm(&mut self) {
        let entry = {
            let c = match self.selected() {
                Some(c) => c,
                None => return,
            };
            let reg = match self.registry.as_ref() {
                Some(r) => r,
                None => return,
            };
            match reg.find_by_id(&c.id) {
                Some(e) => e.clone(),
                None => return,
            }
        };
        self.overlay = Overlay::RemoveConfirm { entry };
    }

    fn cycle_agent(&mut self) {
        if let Overlay::NewWorktree {
            ref mut agent_idx, ..
        } = self.overlay
        {
            *agent_idx = (*agent_idx + 1) % AGENTS.len();
        }
    }

    fn cycle_agent_back(&mut self) {
        if let Overlay::NewWorktree {
            ref mut agent_idx, ..
        } = self.overlay
        {
            *agent_idx = agent_idx.checked_sub(1).unwrap_or(AGENTS.len() - 1);
        }
    }

    fn wt_name_push(&mut self, c: char) {
        if let Overlay::NewWorktree {
            ref mut name_buf, ..
        } = self.overlay
        {
            name_buf.push(c);
        }
    }

    fn wt_name_backspace(&mut self) {
        if let Overlay::NewWorktree {
            ref mut name_buf, ..
        } = self.overlay
        {
            name_buf.pop();
        }
    }

    /// Returns the worktree_path of the newly created worktree, if any.
    fn confirm_new_worktree(&mut self) -> Option<std::path::PathBuf> {
        let (name, agent_kind, repo_root) = match &self.overlay {
            Overlay::NewWorktree {
                name_buf,
                agent_idx,
                repo_root,
            } => {
                let name = if name_buf.is_empty() {
                    None
                } else {
                    Some(name_buf.clone())
                };
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
                return Some(entry.worktree_path);
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

struct RefreshResult {
    containers: Vec<Container>,
    pr_cache: HashMap<String, (Option<PrStatus>, Instant)>,
    selected_log: Option<(String, Vec<vigil_core::LogEvent>, Vec<String>)>,
}

pub async fn run(adapters: AdapterMap) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, adapters).await;

    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
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
            match event::read()? {
                Event::Paste(paste) => {
                    if let Overlay::SendMessage { ref mut input, .. } = app.overlay {
                        input_push_paste(input, paste);
                    }
                }
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    if app.overlay_is_none() {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Down | KeyCode::Char('j') => app.next(),
                            KeyCode::Up | KeyCode::Char('k') => app.prev(),
                            KeyCode::Char('a') => app.cycle_selected_agent(),
                            KeyCode::Char('i') => {
                                if let Some(c) = app.selected() {
                                    app.overlay = Overlay::SendMessage {
                                        input: Vec::new(),
                                        container_id: Some(c.id.clone()),
                                        return_to_log: false,
                                    };
                                }
                            }
                            KeyCode::Char('l') => app.open_log_view(),
                            KeyCode::Char('d') => app.open_dismiss_confirm(),
                            KeyCode::Char('u') => app.undo_dismiss(),
                            KeyCode::Char('W') => app.open_new_worktree(),
                            KeyCode::Char('A') => {
                                if app.registry.is_some() {
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    app.scan_rx = Some(rx);
                                    app.open_project_picker();
                                    tokio::spawn(async move {
                                        tx.send(scan_git_repos().await).ok();
                                    });
                                }
                            }
                            KeyCode::Char('R') => app.open_remove_confirm(),
                            KeyCode::Enter => {
                                if let Some(c) = app.selected() {
                                    let session_id = c.session_id.clone();
                                    let path = c.worktree_path.clone();
                                    let agent = c.agent;
                                    attach_or_launch(
                                        terminal,
                                        &adapters,
                                        session_id.as_ref(),
                                        &path,
                                        agent,
                                    )?;
                                    terminal.clear()?;
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::NewWorktree { .. }) {
                        match key.code {
                            KeyCode::Esc => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Tab => {
                                app.cycle_agent();
                            }
                            KeyCode::BackTab => {
                                app.cycle_agent_back();
                            }
                            KeyCode::Backspace => {
                                app.wt_name_backspace();
                            }
                            KeyCode::Enter => {
                                if let Some(path) = app.confirm_new_worktree() {
                                    // Stay in vigil — set pending_select_path so the new
                                    // worktree is auto-selected once the probe delivers it.
                                    app.pending_select_path = Some(path);
                                    // Force registry reload so the fast-path picks up the entry.
                                    app.registry = Registry::load().ok();
                                }
                            }
                            KeyCode::Char(c) => {
                                app.wt_name_push(c);
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::LogView { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('q') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Char('i') => {
                                let container_id =
                                    if let Overlay::LogView { container_id, .. } = &app.overlay {
                                        Some(container_id.clone())
                                    } else {
                                        None
                                    };
                                app.overlay = Overlay::SendMessage {
                                    input: Vec::new(),
                                    container_id,
                                    return_to_log: true,
                                };
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                if let Overlay::LogView { ref mut scroll, .. } = app.overlay {
                                    *scroll = scroll.saturating_add(3);
                                }
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                if let Overlay::LogView { ref mut scroll, .. } = app.overlay {
                                    *scroll = scroll.saturating_sub(3);
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::DismissConfirm { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.confirm_dismiss();
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::RemoveConfirm { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.confirm_remove();
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::ProjectPicker { .. }) {
                        match key.code {
                            KeyCode::Esc => {
                                app.overlay = Overlay::None;
                                app.scan_rx = None;
                            }
                            KeyCode::Backspace => {
                                if let Overlay::ProjectPicker {
                                    ref mut query,
                                    ref mut selected_idx,
                                    ..
                                } = app.overlay
                                {
                                    query.pop();
                                    *selected_idx = 0;
                                }
                            }
                            KeyCode::Down | KeyCode::Tab => {
                                let count = picker_filtered_count(&app.overlay);
                                if let Overlay::ProjectPicker {
                                    ref mut selected_idx,
                                    ..
                                } = app.overlay
                                {
                                    *selected_idx =
                                        (*selected_idx + 1).min(count.saturating_sub(1));
                                }
                            }
                            KeyCode::Up => {
                                if let Overlay::ProjectPicker {
                                    ref mut selected_idx,
                                    ..
                                } = app.overlay
                                {
                                    *selected_idx = selected_idx.saturating_sub(1);
                                }
                            }
                            KeyCode::Enter => {
                                // Extract selected repo, then transition to NewWorktree
                                let selected = if let Overlay::ProjectPicker {
                                    ref all_repos,
                                    ref query,
                                    selected_idx,
                                    ..
                                } = app.overlay
                                {
                                    let q = query.to_lowercase();
                                    let filtered: Vec<&PathBuf> = all_repos
                                        .iter()
                                        .filter(|p| {
                                            p.display().to_string().to_lowercase().contains(&q)
                                        })
                                        .collect();
                                    filtered.get(selected_idx).map(|p| (*p).clone())
                                } else {
                                    None
                                };
                                if let Some(repo_root) = selected {
                                    app.scan_rx = None;
                                    app.overlay = Overlay::NewWorktree {
                                        name_buf: String::new(),
                                        agent_idx: 0,
                                        repo_root: Some(repo_root),
                                    };
                                }
                            }
                            KeyCode::Char(c) => {
                                if let Overlay::ProjectPicker {
                                    ref mut query,
                                    ref mut selected_idx,
                                    ..
                                } = app.overlay
                                {
                                    query.push(c);
                                    *selected_idx = 0;
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::SendMessage { .. }) {
                        match key.code {
                            KeyCode::Esc => {
                                close_send_message(&mut app);
                            }
                            KeyCode::Backspace => {
                                if let Overlay::SendMessage { ref mut input, .. } = app.overlay {
                                    input_backspace(input);
                                }
                            }
                            KeyCode::Enter => {
                                let is_newline = key.modifiers.contains(KeyModifiers::CONTROL)
                                    || key.modifiers.contains(KeyModifiers::ALT);
                                if is_newline {
                                    if let Overlay::SendMessage { ref mut input, .. } = app.overlay
                                    {
                                        input_push_char(input, '\n');
                                    }
                                } else {
                                    let (msg, container_id, return_to_log) =
                                        if let Overlay::SendMessage {
                                            input,
                                            container_id,
                                            return_to_log,
                                        } = &app.overlay
                                        {
                                            (
                                                input_text(input),
                                                container_id.clone(),
                                                *return_to_log,
                                            )
                                        } else {
                                            unreachable!()
                                        };
                                    let target = container_id
                                        .as_ref()
                                        .and_then(|id| app.containers.iter().find(|c| &c.id == id))
                                        .or_else(|| app.selected());
                                    let dir = target.map(|c| c.worktree_path.clone());
                                    let sid = target.and_then(|c| c.session_id.clone());
                                    let agent = target.map(|c| c.agent);
                                    app.overlay = if return_to_log {
                                        container_id
                                            .as_ref()
                                            .map(|id| log_view_overlay_from_cache(&app, id))
                                            .unwrap_or(Overlay::None)
                                    } else {
                                        Overlay::None
                                    };
                                    if let Some(dir) = dir {
                                        if let Some(agent) = agent {
                                            if let Some(adapter) = adapters.get(&agent) {
                                                if let Some(sid) = sid {
                                                    adapter
                                                        .send_message(&dir, &sid, &msg)
                                                        .await
                                                        .ok();
                                                } else {
                                                    // No session yet — start a fresh one with this message.
                                                    adapter
                                                        .start_with_message(&dir, &msg)
                                                        .await
                                                        .ok();
                                                }
                                            }
                                        }
                                    }
                                    // Persist the sent message so refresh() can apply it every tick.
                                    // history.jsonl is not updated by --print mode, so the probe would
                                    // otherwise revert the display to the previous message.
                                    if !msg.is_empty() {
                                        if let Some(id) = container_id {
                                            app.last_sent.insert(id, msg);
                                        }
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                if let Overlay::SendMessage { ref mut input, .. } = app.overlay {
                                    input_push_char(input, c);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if app.last_refresh.elapsed() >= TICK {
            refresh(&mut app, &adapters).await;
        }
    }

    Ok(())
}

fn input_push_char(input: &mut Vec<MessageInputChunk>, c: char) {
    match input.last_mut() {
        Some(MessageInputChunk::Text(text)) => text.push(c),
        _ => input.push(MessageInputChunk::Text(c.to_string())),
    }
}

fn input_push_paste(input: &mut Vec<MessageInputChunk>, text: String) {
    if !text.is_empty() {
        input.push(MessageInputChunk::Paste(text));
    }
}

fn input_backspace(input: &mut Vec<MessageInputChunk>) {
    match input.last_mut() {
        Some(MessageInputChunk::Text(text)) => {
            text.pop();
            if text.is_empty() {
                input.pop();
            }
        }
        Some(MessageInputChunk::Paste(_)) => {
            input.pop();
        }
        None => {}
    }
}

pub fn input_text(input: &[MessageInputChunk]) -> String {
    let mut text = String::new();
    for chunk in input {
        match chunk {
            MessageInputChunk::Text(part) | MessageInputChunk::Paste(part) => text.push_str(part),
        }
    }
    text
}

pub fn input_presentation(input: &[MessageInputChunk]) -> String {
    let mut text = String::new();
    for chunk in input {
        match chunk {
            MessageInputChunk::Text(part) => text.push_str(part),
            MessageInputChunk::Paste(part) => {
                let lines = part.lines().count().max(1);
                let suffix = if lines == 1 { "" } else { "s" };
                text.push_str(&format!("[pasted {lines} line{suffix}]"));
            }
        }
    }
    text
}

fn close_send_message(app: &mut App) {
    let (return_to_log, container_id) = if let Overlay::SendMessage {
        container_id,
        return_to_log,
        ..
    } = &app.overlay
    {
        (*return_to_log, container_id.clone())
    } else {
        (false, None)
    };

    app.overlay = if return_to_log {
        container_id
            .as_ref()
            .map(|id| log_view_overlay_from_cache(app, id))
            .unwrap_or(Overlay::None)
    } else {
        Overlay::None
    };
}

fn log_view_overlay_from_cache(app: &App, container_id: &str) -> Overlay {
    let (events, lines) = app.log_cache.get(container_id).cloned().unwrap_or_default();
    Overlay::LogView {
        container_id: container_id.to_string(),
        events,
        lines,
        scroll: 0,
    }
}

async fn refresh(app: &mut App, adapters: &AdapterMap) {
    app.registry = Registry::load().ok();

    let selected_id = app.selected().map(|c| c.id.clone());

    if let Some(rx) = app.refresh_rx.as_mut() {
        match rx.try_recv() {
            Ok(result) => {
                app.refresh_rx = None;
                app.pr_cache = result.pr_cache;
                if let Some((id, events, lines)) = result.selected_log {
                    app.log_cache.insert(id, (events, lines));
                }
                app.containers = result.containers;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                app.refresh_rx = None;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
    }

    // Reconcile cached rows against the current registry/archive immediately. The
    // expensive part (adapter probing, PR lookups, logs) happens in the background;
    // this cheap diff keeps deleted/dismissed rows from hanging around while that
    // work is in flight.
    let live_ids: std::collections::HashSet<String> = match &app.registry {
        Some(reg) => reg
            .entries()
            .iter()
            .filter(|e| !app.archive.is_dismissed_id(&e.id))
            .map(|e| e.id.clone())
            .collect(),
        None => std::collections::HashSet::new(),
    };
    app.containers.retain(|c| live_ids.contains(&c.id));
    sort_containers(&mut app.containers);

    // If we're waiting to auto-select a freshly-created worktree, check if it
    // has now appeared in the container list (i.e. background probe completed).
    let pending_id = app
        .pending_select_path
        .as_ref()
        .and_then(|path| app.containers.iter().find(|c| &c.worktree_path == path))
        .map(|c| c.id.clone());
    if pending_id.is_some() {
        app.pending_select_path = None;
    }
    restore_selection(app, pending_id.or(selected_id));

    // Deliver scan results to ProjectPicker if ready.
    let scan_ready = app.scan_rx.as_mut().and_then(|rx| rx.try_recv().ok());
    if let Some(mut results) = scan_ready {
        app.scan_rx = None;
        results.sort_by_key(|a| a.display().to_string());
        if let Overlay::ProjectPicker {
            ref mut all_repos,
            ref mut scanning,
            ref mut selected_idx,
            ..
        } = app.overlay
        {
            *all_repos = results;
            *scanning = false;
            *selected_idx = 0;
        }
    }

    // Keep LogView in sync from cache while it is open. If the cache is missing,
    // the background refresh will populate it rather than blocking this frame.
    if let Overlay::LogView {
        container_id,
        events,
        lines,
        scroll,
        ..
    } = &mut app.overlay
    {
        if let Some((cached_events, cached_lines)) = app.log_cache.get(container_id) {
            *events = cached_events.clone();
            *lines = cached_lines.clone();
            if events.is_empty() && lines.is_empty() {
                *scroll = 0;
            }
        }
    }

    if app.refresh_rx.is_none() {
        let entries: Vec<WorktreeEntry> = match &app.registry {
            Some(reg) => reg
                .entries()
                .iter()
                .filter(|e| !app.archive.is_dismissed_id(&e.id))
                .cloned()
                .collect(),
            None => vec![],
        };
        let adapters = adapters.clone();
        let pr_cache = app.pr_cache.clone();
        let last_sent = app.last_sent.clone();
        let selected_log_info = app
            .selected()
            .map(|c| (c.id.clone(), c.worktree_path.clone(), c.agent));
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.refresh_rx = Some(rx);
        tokio::spawn(async move {
            let result =
                build_refresh_result(entries, adapters, pr_cache, last_sent, selected_log_info)
                    .await;
            tx.send(result).ok();
        });
    }

    app.last_refresh = Instant::now();
}

async fn build_refresh_result(
    entries: Vec<WorktreeEntry>,
    adapters: AdapterMap,
    mut pr_cache: HashMap<String, (Option<PrStatus>, Instant)>,
    last_sent: HashMap<String, String>,
    selected_log_info: Option<(String, PathBuf, AgentKind)>,
) -> RefreshResult {
    let mut handles = Vec::with_capacity(entries.len());
    for entry in &entries {
        let path = entry.worktree_path.clone();
        let adapter = adapters
            .get(&entry.agent)
            .or_else(|| adapters.values().next())
            .map(Arc::clone);
        handles.push(tokio::spawn(async move {
            match adapter {
                Some(a) => a.probe(&path).await,
                None => ProbeResult::no_session(),
            }
        }));
    }

    let mut pr_handles: Vec<(String, _)> = Vec::new();
    for entry in &entries {
        let cached = pr_cache.get(&entry.id);
        let stale = cached.is_none_or(|(_, t)| t.elapsed() >= PR_REFRESH);
        if stale {
            let repo_root = entry.repo_root.clone();
            let branch = entry.branch.clone();
            let id = entry.id.clone();
            pr_handles.push((
                id,
                tokio::spawn(async move { probe_pr(&repo_root, &branch).await }),
            ));
        }
    }
    for (id, handle) in pr_handles {
        let status = handle.await.unwrap_or(None);
        pr_cache.insert(id, (status, Instant::now()));
    }

    let mut containers = Vec::with_capacity(entries.len());
    for (entry, handle) in entries.iter().zip(handles) {
        let probe: ProbeResult = handle.await.unwrap_or_else(|_| ProbeResult::no_session());
        let pr_status = pr_cache.get(&entry.id).and_then(|(s, _)| s.clone());
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
            last_user_message: last_sent
                .get(&entry.id)
                .cloned()
                .or(probe.last_user_message),
            pr_status,
        });
    }
    sort_containers(&mut containers);

    let selected_log = match selected_log_info {
        Some((id, path, agent)) => match adapters.get(&agent) {
            Some(adapter) => {
                let events = adapter.recent_log_events(&path).await;
                let lines = adapter.recent_log(&path).await;
                Some((id, events, lines))
            }
            None => None,
        },
        None => None,
    };

    RefreshResult {
        containers,
        pr_cache,
        selected_log,
    }
}

fn sort_containers(containers: &mut [Container]) {
    let vigil_wt = vigil_worktrees_prefix();
    let mut group_index: HashMap<String, usize> = HashMap::new();
    for c in containers.iter() {
        let g = container_repo_group(c, &vigil_wt);
        let next = group_index.len();
        group_index.entry(g).or_insert(next);
    }
    containers.sort_by_key(|c| group_index[&container_repo_group(c, &vigil_wt)]);
}

fn restore_selection(app: &mut App, selected_id: Option<String>) {
    let new_sel = selected_id
        .and_then(|id| app.containers.iter().position(|c| c.id == id))
        .unwrap_or(app.table_state.selected().unwrap_or(0))
        .min(app.containers.len().saturating_sub(1));

    if app.containers.is_empty() {
        app.table_state.select(None);
    } else {
        app.table_state.select(Some(new_sel));
    }
}

/// Query `gh pr view` to get PR status for a branch. Returns None if gh is unavailable.
async fn probe_pr(repo_root: &std::path::Path, branch: &str) -> Option<PrStatus> {
    let output = tokio::process::Command::new("gh")
        .args(["pr", "view", branch, "--json", "state,mergeStateStatus"])
        .current_dir(repo_root)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return Some(PrStatus::NoPr);
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    match json["state"].as_str()? {
        "MERGED" => Some(PrStatus::Merged),
        "OPEN" => {
            if json["mergeStateStatus"].as_str() == Some("CLEAN") {
                Some(PrStatus::ReadyToMerge)
            } else {
                Some(PrStatus::InProgress)
            }
        }
        _ => Some(PrStatus::NoPr),
    }
}

/// Count filtered results for the ProjectPicker without taking ownership.
fn picker_filtered_count(overlay: &Overlay) -> usize {
    if let Overlay::ProjectPicker {
        all_repos, query, ..
    } = overlay
    {
        let q = query.to_lowercase();
        all_repos
            .iter()
            .filter(|p| p.display().to_string().to_lowercase().contains(&q))
            .count()
    } else {
        0
    }
}

fn repo_scan_cache_path() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".vigil").join("repo-scan-cache.json"))
        .unwrap_or_else(|| PathBuf::from("~/.vigil/repo-scan-cache.json"))
}

fn current_search_dirs() -> Vec<PathBuf> {
    crate::config::Config::load().search_paths()
}

fn load_repo_scan_cache() -> Vec<PathBuf> {
    let dirs = current_search_dirs();
    std::fs::read_to_string(repo_scan_cache_path())
        .ok()
        .and_then(|s| serde_json::from_str::<RepoScanCache>(&s).ok())
        .filter(|cache| cache.search_dirs == dirs)
        .map(|cache| cache.repos)
        .unwrap_or_default()
}

fn save_repo_scan_cache(search_dirs: Vec<PathBuf>, repos: Vec<PathBuf>) {
    let path = repo_scan_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = RepoScanCache { search_dirs, repos };
    if let Ok(json) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(path, json);
    }
}

/// Scan configured directories for git repositories using mdfind (macOS Spotlight) or find.
async fn scan_git_repos() -> Vec<PathBuf> {
    let dirs = current_search_dirs();
    if dirs.is_empty() {
        return vec![];
    }

    let mut all_stdout = Vec::new();

    for dir in &dirs {
        let dir_str = dir.display().to_string();

        // Try mdfind first — fast Spotlight index query, typically < 200ms on macOS.
        let mdfind = tokio::process::Command::new("mdfind")
            .args(["-onlyin", &dir_str, "kMDItemFSName == '.git'"])
            .output()
            .await;

        let stdout = match mdfind {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => out.stdout,
            _ => {
                // Fallback: find with depth limit, skipping common noise dirs.
                tokio::process::Command::new("find")
                    .arg(&dir_str)
                    .args([
                        "-maxdepth",
                        "5",
                        "-type",
                        "d",
                        "-name",
                        ".git",
                        "-not",
                        "-path",
                        "*/node_modules/*",
                        "-not",
                        "-path",
                        "*/.Trash/*",
                    ])
                    .output()
                    .await
                    .map(|o| o.stdout)
                    .unwrap_or_default()
            }
        };
        all_stdout.extend_from_slice(&stdout);
    }

    let mut seen = std::collections::HashSet::new();
    let mut repos: Vec<PathBuf> = String::from_utf8_lossy(&all_stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // Skip .git dirs that live inside another .git (submodule metadata)
            if line.contains("/.git/") {
                return None;
            }
            let git_dir = PathBuf::from(line);
            git_dir.parent().map(|p| p.to_path_buf())
        })
        .filter(|p| seen.insert(p.clone()))
        .collect();

    repos.sort_by_key(|a| a.display().to_string());
    save_repo_scan_cache(dirs, repos.clone());
    repos
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
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
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
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;

    Ok(())
}
