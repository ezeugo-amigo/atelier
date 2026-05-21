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
use vigil_core::{AgentAdapter, AgentKind, Container, LogEvent, ProbeResult, PrStatus, SessionId};

use crate::archive::Archive;

const TICK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);
const PR_REFRESH: Duration = Duration::from_secs(60);

const AGENTS: [AgentKind; 4] = [
    AgentKind::ClaudeCode,
    AgentKind::Codex,
    AgentKind::Pi,
    AgentKind::OpenCode,
];

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
        c.repo_root.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()
    }
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
        worktree_path: PathBuf,
        agent: AgentKind,
        /// Structured turn events (Pi and others with JSONL sessions).
        events: Vec<LogEvent>,
        /// Raw log lines fallback (Claude Code debug logs).
        lines: Vec<String>,
    },
    SendMessage {
        buf: String,
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
    /// Last message sent via the SendMessage overlay, keyed by container id.
    /// Persists across refresh() cycles since history.jsonl isn't updated by --print mode.
    last_sent: HashMap<String, String>,
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
            last_sent: HashMap::new(),
        }
    }

    pub fn selected(&self) -> Option<&Container> {
        self.table_state
            .selected()
            .and_then(|i| self.containers.get(i))
    }

    pub fn selected_dir(&self) -> Option<&PathBuf> {
        self.selected().map(|c| &c.worktree_path)
    }

    pub fn selected_session_id(&self) -> Option<&SessionId> {
        self.selected().and_then(|c| c.session_id.as_ref())
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

    pub fn cycle_selected_agent(&mut self) {
        let Some(c) = self.selected() else { return };
        let id = c.id.clone();
        let current = c.agent;
        let next = match current {
            AgentKind::ClaudeCode => AgentKind::Codex,
            AgentKind::Codex      => AgentKind::Pi,
            AgentKind::Pi         => AgentKind::OpenCode,
            AgentKind::OpenCode   => AgentKind::ClaudeCode,
        };
        if let Some(registry) = self.registry.as_mut() {
            registry.update_agent(&id, next).ok();
        }
    }

    pub fn open_log_view(&mut self) {
        if let Some(c) = self.selected() {
            let (events, lines) = self.log_cache.get(&c.id).cloned().unwrap_or_default();
            self.overlay = Overlay::LogView {
                container_id: c.id.clone(),
                worktree_path: c.worktree_path.clone(),
                agent: c.agent,
                events,
                lines,
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

    pub fn open_project_picker(&mut self) {
        if self.registry.is_none() { return; }
        self.overlay = Overlay::ProjectPicker {
            query: String::new(),
            all_repos: vec![],
            selected_idx: 0,
            scanning: true,
        };
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
                        KeyCode::Char('a') => app.cycle_selected_agent(),
                        KeyCode::Char('i') => {
                            if app.selected().is_some() {
                                app.overlay = Overlay::SendMessage { buf: String::new() };
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
                                tokio::spawn(async move { tx.send(scan_git_repos().await).ok(); });
                            }
                        }
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
                } else if matches!(app.overlay, Overlay::ProjectPicker { .. }) {
                    match key.code {
                        KeyCode::Esc => {
                            app.overlay = Overlay::None;
                            app.scan_rx = None;
                        }
                        KeyCode::Backspace => {
                            if let Overlay::ProjectPicker { ref mut query, ref mut selected_idx, .. } = app.overlay {
                                query.pop();
                                *selected_idx = 0;
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            let count = picker_filtered_count(&app.overlay);
                            if let Overlay::ProjectPicker { ref mut selected_idx, .. } = app.overlay {
                                *selected_idx = (*selected_idx + 1).min(count.saturating_sub(1));
                            }
                        }
                        KeyCode::Up => {
                            if let Overlay::ProjectPicker { ref mut selected_idx, .. } = app.overlay {
                                *selected_idx = selected_idx.saturating_sub(1);
                            }
                        }
                        KeyCode::Enter => {
                            // Extract selected repo, then transition to NewWorktree
                            let selected = if let Overlay::ProjectPicker { ref all_repos, ref query, selected_idx, .. } = app.overlay {
                                let q = query.to_lowercase();
                                let filtered: Vec<&PathBuf> = all_repos.iter()
                                    .filter(|p| p.display().to_string().to_lowercase().contains(&q))
                                    .collect();
                                filtered.get(selected_idx).map(|p| (*p).clone())
                            } else { None };
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
                            if let Overlay::ProjectPicker { ref mut query, ref mut selected_idx, .. } = app.overlay {
                                query.push(c);
                                *selected_idx = 0;
                            }
                        }
                        _ => {}
                    }
                } else if matches!(app.overlay, Overlay::SendMessage { .. }) {
                    match key.code {
                        KeyCode::Esc => { app.overlay = Overlay::None; }
                        KeyCode::Backspace => {
                            if let Overlay::SendMessage { ref mut buf } = app.overlay {
                                buf.pop();
                            }
                        }
                        KeyCode::Enter => {
                            let (msg, dir, sid) = if let Overlay::SendMessage { ref buf } = app.overlay {
                                let msg = buf.clone();
                                let dir = app.selected_dir().cloned();
                                let sid = app.selected_session_id().cloned();
                                (msg, dir, sid)
                            } else {
                                unreachable!()
                            };
                            let container_id = app.selected().map(|c| c.id.clone());
                            app.overlay = Overlay::None;
                            if let (Some(dir), Some(sid)) = (dir, sid) {
                                let agent = app.selected().map(|c| c.agent);
                                if let Some(agent) = agent {
                                    if let Some(adapter) = adapters.get(&agent) {
                                        let _ = adapter.send_message(&dir, &sid, &msg).await;
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
                        KeyCode::Char(c) => {
                            if let Overlay::SendMessage { ref mut buf } = app.overlay {
                                buf.push(c);
                            }
                        }
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

    // Spawn stale PR probes in parallel
    let mut pr_handles: Vec<(String, _)> = Vec::new();
    for entry in &entries {
        let cached = app.pr_cache.get(&entry.id);
        let stale = cached.map_or(true, |(_, t)| t.elapsed() >= PR_REFRESH);
        if stale {
            let repo_root = entry.repo_root.clone();
            let branch = entry.branch.clone();
            let id = entry.id.clone();
            pr_handles.push((id, tokio::spawn(async move {
                probe_pr(&repo_root, &branch).await
            })));
        }
    }
    for (id, handle) in pr_handles {
        let status = handle.await.unwrap_or(None);
        app.pr_cache.insert(id, (status, Instant::now()));
    }

    for (entry, handle) in entries.iter().zip(handles) {
        let probe: ProbeResult = handle.await.unwrap_or_else(|_| ProbeResult::no_session());
        let pr_status = app.pr_cache.get(&entry.id).and_then(|(s, _)| s.clone());
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
            last_user_message: app.last_sent
                .get(&entry.id)
                .cloned()
                .or(probe.last_user_message),
            pr_status,
        });
    }

    // Sort containers to match the visual grouping in render.rs (by repo, preserving
    // first-seen order). Without this, j/k navigates in registry creation order rather
    // than the order rows actually appear on screen.
    {
        let vigil_wt = vigil_worktrees_prefix();
        let mut group_index: HashMap<String, usize> = HashMap::new();
        for c in &containers {
            let g = container_repo_group(c, &vigil_wt);
            let next = group_index.len();
            group_index.entry(g).or_insert(next);
        }
        containers.sort_by_key(|c| group_index[&container_repo_group(c, &vigil_wt)]);
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

    // Deliver scan results to ProjectPicker if ready.
    let scan_ready = app.scan_rx.as_mut().and_then(|rx| rx.try_recv().ok());
    if let Some(mut results) = scan_ready {
        app.scan_rx = None;
        results.sort_by(|a, b| a.display().to_string().cmp(&b.display().to_string()));
        if let Overlay::ProjectPicker { ref mut all_repos, ref mut scanning, ref mut selected_idx, .. } = app.overlay {
            *all_repos = results;
            *scanning = false;
            *selected_idx = 0;
        }
    }

    // Pre-load log for the selected container into cache (so opening LogView is instant).
    let selected_log_info = app.selected().map(|c| (c.id.clone(), c.worktree_path.clone(), c.agent));
    if let Some((id, path, agent)) = selected_log_info {
        if let Some(adapter) = adapters.get(&agent) {
            let events = adapter.recent_log_events(&path).await;
            let lines = adapter.recent_log(&path).await;
            app.log_cache.insert(id, (events, lines));
        }
    }

    // Keep LogView in sync while it's open.
    if let Overlay::LogView { container_id, worktree_path, agent, events, lines, .. } = &mut app.overlay {
        if let Some((cached_events, cached_lines)) = app.log_cache.get(container_id) {
            *events = cached_events.clone();
            *lines = cached_lines.clone();
        } else if let Some(adapter) = adapters.get(agent) {
            *events = adapter.recent_log_events(worktree_path).await;
            *lines = adapter.recent_log(worktree_path).await;
        }
    }

    app.last_refresh = Instant::now();
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
    if let Overlay::ProjectPicker { all_repos, query, .. } = overlay {
        let q = query.to_lowercase();
        all_repos.iter().filter(|p| p.display().to_string().to_lowercase().contains(&q)).count()
    } else { 0 }
}

/// Scan the home directory for git repositories using mdfind (macOS Spotlight) or find.
async fn scan_git_repos() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() { return vec![]; }

    // Try mdfind first — fast Spotlight index query, typically < 200ms on macOS.
    let mdfind = tokio::process::Command::new("mdfind")
        .args(["-onlyin", &home, "kMDItemFSName == '.git'"])
        .output()
        .await;

    let stdout = match mdfind {
        Ok(out) if out.status.success() && !out.stdout.is_empty() => out.stdout,
        _ => {
            // Fallback: find with depth limit, skipping common noise dirs.
            tokio::process::Command::new("find")
                .arg(&home)
                .args(["-maxdepth", "5", "-type", "d", "-name", ".git",
                       "-not", "-path", "*/node_modules/*",
                       "-not", "-path", "*/.Trash/*"])
                .output()
                .await
                .map(|o| o.stdout)
                .unwrap_or_default()
        }
    };

    String::from_utf8_lossy(&stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() { return None; }
            // Skip .git dirs that live inside another .git (submodule metadata)
            if line.contains("/.git/") { return None; }
            let git_dir = PathBuf::from(line);
            git_dir.parent().map(|p| p.to_path_buf())
        })
        .collect()
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
