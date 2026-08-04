use std::collections::HashMap;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
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
use vigil_core::{
    aggregate_pr_status, process::cwd_snapshot, AgentAdapter, AgentKind, BackgroundProcess,
    Container, LogEvent, PrStatus, ProbeResult, RepoStatus, SessionId, SessionState,
};

use crate::archive::Archive;
use crate::greeting::Greeting;
use crate::recap::Recap;
use crate::scratch::{ScratchChat, ScratchStore};

const TICK: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(100);
const PR_REFRESH: Duration = Duration::from_secs(60);
const BRANCH_REFRESH: Duration = Duration::from_secs(5);
const GREETING_VISIBLE_FOR: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize, Deserialize)]
struct RepoScanCache {
    search_dirs: Vec<PathBuf>,
    repos: Vec<PathBuf>,
}

pub fn agents() -> Vec<AgentKind> {
    AgentKind::all()
        .iter()
        .copied()
        .filter(|a| a.available())
        .collect()
}

fn vigil_worktrees_prefix() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        "/.vigil/worktrees/".to_string()
    } else {
        format!("{home}/.vigil/worktrees/")
    }
}

fn launch_target(
    containers: &[Container],
    container_id: &str,
) -> Option<(Option<SessionId>, PathBuf, AgentKind)> {
    containers
        .iter()
        .find(|container| container.id == container_id)
        .map(|container| {
            (
                container.session_id.clone(),
                container.worktree_path.clone(),
                container.agent,
            )
        })
}

fn scratch_container(chat: &ScratchChat) -> Container {
    Container {
        id: chat.id.clone(),
        display_name: Some(chat.title.clone()),
        worktree_path: chat.workdir.clone(),
        repo_root: chat.workdir.clone(),
        agent: chat.agent,
        branch: "scratch".to_string(),
        created_at: chat.created_at,
        state: SessionState::NoSession,
        session_id: None,
        last_activity: None,
        last_user_message: None,
        pr_status: None,
        pr_url: None,
        background_processes: Vec::new(),
        repos: Vec::new(),
        is_scratch: true,
    }
}

pub(crate) fn container_repo_group(c: &Container, vigil_wt: &str) -> String {
    if c.is_scratch {
        return "scratch".to_string();
    }
    // Multi-repo workspaces group under the joined repo names.
    if !c.repos.is_empty() {
        return c
            .repos
            .iter()
            .map(|r| {
                r.repo_root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
            })
            .collect::<Vec<_>>()
            .join(" + ");
    }
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

/// Cache key for per-checkout branch/PR probes: a workspace entry has one
/// cache slot per contained repo.
fn probe_cache_key(id: &str, repo_root: &Path) -> String {
    format!("{id}::{}", repo_root.display())
}

pub enum MessageInputChunk {
    Text(String),
    Paste(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardSection {
    Workspaces,
    Scratch,
}

pub enum Overlay {
    None,
    NewWorktree {
        name_buf: String,
        agent: AgentKind,
        /// Repos to check out. Empty = infer from cwd; 2+ = multi-repo workspace.
        repo_roots: Vec<PathBuf>,
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
        /// LLM-generated recap pinned to the corner; synced from `App::recap`.
        recap: Recap,
        /// Whether the recap box is shown; toggled with `R`, persisted in `App`.
        recap_visible: bool,
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
        /// Repos toggled with Space (insertion order). 2+ checked repos create
        /// a multi-repo workspace; empty falls back to the highlighted repo.
        checked: Vec<PathBuf>,
    },
    /// Pick the default agent for new containers; persisted to config.
    DefaultAgent {
        agent: AgentKind,
    },
    /// Create a chat that is not backed by a Git worktree.
    NewScratchChat {
        title_buf: String,
        agent: AgentKind,
    },
    ScratchDeleteConfirm {
        container_id: String,
    },
}

pub struct App {
    pub containers: Vec<Container>,
    pub table_state: TableState,
    pub scratch_table_state: TableState,
    pub section: DashboardSection,
    pub archive: Archive,
    pub scratch: ScratchStore,
    pub overlay: Overlay,
    pub registry: Option<Registry>,
    last_refresh: Instant,
    last_dismissed: Option<Container>,
    /// Cache of PR status and URL per checkout, with the time they were last fetched.
    pr_cache: HashMap<String, (Option<PrProbe>, Instant)>,
    /// Cache of the live git branch per container id, with the time it was last
    /// read. Polled on a slower cadence than the tick since branches rarely change.
    branch_cache: HashMap<String, (String, Instant)>,
    /// Cache of log data per container id, updated every tick for the selected container.
    log_cache: HashMap<String, (Vec<vigil_core::LogEvent>, Vec<String>)>,
    /// Cache of the last recap per container id, persisted across log-view open/close.
    recap: HashMap<String, Recap>,
    /// Receiver for an in-flight recap generation (delivers (container_id, result)).
    recap_rx: Option<tokio::sync::oneshot::Receiver<(String, Result<String, String>)>>,
    /// Whether the recap box is shown in the log view; persists across open/close.
    recap_visible: bool,
    /// The one-shot startup greeting, shown briefly in the top-right corner.
    greeting: Option<Greeting>,
    /// Name used for the deferred model enhancement of the startup greeting.
    greeting_name: Option<String>,
    /// Receiver for the async startup greeting generation.
    greeting_rx: Option<tokio::sync::oneshot::Receiver<(String, Result<String, String>)>>,
    /// Whether the startup greeting card is currently visible.
    greeting_visible: bool,
    /// When the completed greeting card should disappear.
    greeting_deadline: Option<Instant>,
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
    /// After creating a scratch chat, select it once the next refresh delivers it.
    pending_select_id: Option<String>,
    /// Agent changes made in the UI that may not be reflected in an in-flight
    /// refresh snapshot yet.
    pending_agent_overrides: HashMap<String, AgentKind>,
    /// Default agent kind for new containers, sourced from `~/.vigil/config.json`.
    default_agent: AgentKind,
}

impl App {
    fn new() -> Self {
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        Self {
            containers: Vec::new(),
            table_state,
            scratch_table_state: TableState::default(),
            section: DashboardSection::Workspaces,
            archive: Archive::load(),
            scratch: ScratchStore::load(),
            overlay: Overlay::None,
            registry: Registry::load().ok(),
            last_refresh: Instant::now(),
            last_dismissed: None,
            log_cache: HashMap::new(),
            recap: HashMap::new(),
            recap_rx: None,
            recap_visible: true,
            greeting: None,
            greeting_name: None,
            greeting_rx: None,
            greeting_visible: false,
            greeting_deadline: None,
            pr_cache: HashMap::new(),
            branch_cache: HashMap::new(),
            scan_rx: None,
            refresh_rx: None,
            last_sent: HashMap::new(),
            pending_select_path: None,
            pending_select_id: None,
            pending_agent_overrides: HashMap::new(),
            default_agent: crate::config::Config::load().default_agent(),
        }
    }

    fn next_agent(current: AgentKind) -> AgentKind {
        let all = agents();
        let idx = all.iter().position(|a| *a == current).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    fn prev_agent(current: AgentKind) -> AgentKind {
        let all = agents();
        let idx = all.iter().position(|a| *a == current).unwrap_or(0);
        all[idx.checked_sub(1).unwrap_or(all.len() - 1)]
    }

    pub fn selected(&self) -> Option<&Container> {
        self.selected_container_index()
            .and_then(|index| self.containers.get(index))
    }

    pub(crate) fn workspace_indices(&self) -> Vec<usize> {
        self.containers
            .iter()
            .enumerate()
            .filter_map(|(index, container)| (!container.is_scratch).then_some(index))
            .collect()
    }

    pub(crate) fn scratch_indices(&self) -> Vec<usize> {
        self.containers
            .iter()
            .enumerate()
            .filter_map(|(index, container)| container.is_scratch.then_some(index))
            .collect()
    }

    fn selected_container_index(&self) -> Option<usize> {
        let indices = match self.section {
            DashboardSection::Workspaces => self.workspace_indices(),
            DashboardSection::Scratch => self.scratch_indices(),
        };
        let selected = match self.section {
            DashboardSection::Workspaces => self.table_state.selected(),
            DashboardSection::Scratch => self.scratch_table_state.selected(),
        }?;
        indices.get(selected).copied()
    }

    fn section_count(&self) -> usize {
        match self.section {
            DashboardSection::Workspaces => self.workspace_indices().len(),
            DashboardSection::Scratch => self.scratch_indices().len(),
        }
    }

    fn section_state_mut(&mut self) -> &mut TableState {
        match self.section {
            DashboardSection::Workspaces => &mut self.table_state,
            DashboardSection::Scratch => &mut self.scratch_table_state,
        }
    }

    pub fn focus_workspaces(&mut self) {
        self.section = DashboardSection::Workspaces;
        if self.table_state.selected().is_none() && !self.workspace_indices().is_empty() {
            self.table_state.select(Some(0));
        }
    }

    pub fn focus_scratch(&mut self) {
        self.section = DashboardSection::Scratch;
        if self.scratch_table_state.selected().is_none() && !self.scratch_indices().is_empty() {
            self.scratch_table_state.select(Some(0));
        }
    }

    fn clamp_section_selection(&mut self) {
        let count = self.section_count();
        let state = self.section_state_mut();
        if count == 0 {
            state.select(None);
        } else {
            let selected = state.selected().unwrap_or(0).min(count - 1);
            state.select(Some(selected));
        }
    }

    fn select_id_in_section(&mut self, id: &str, section: DashboardSection) {
        self.section = section;
        let indices = match section {
            DashboardSection::Workspaces => self.workspace_indices(),
            DashboardSection::Scratch => self.scratch_indices(),
        };
        let selection = indices
            .iter()
            .position(|index| self.containers[*index].id == id);
        self.section_state_mut().select(selection);
    }

    pub fn overlay_is_none(&self) -> bool {
        matches!(self.overlay, Overlay::None)
    }

    pub fn visible_greeting(&self) -> Option<&Greeting> {
        self.greeting_visible
            .then(|| self.greeting.as_ref())
            .flatten()
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
        let count = self.section_count();
        if count == 0 {
            return;
        }
        let i = self.section_state_mut().selected().unwrap_or(0);
        self.section_state_mut()
            .select(Some((i + 1).min(count - 1)));
    }

    fn prev(&mut self) {
        let i = self.section_state_mut().selected().unwrap_or(0);
        self.section_state_mut().select(Some(i.saturating_sub(1)));
    }

    pub fn cycle_selected_agent(&mut self) {
        let Some(i) = self.selected_container_index() else {
            return;
        };
        let Some(c) = self.containers.get_mut(i) else {
            return;
        };
        let id = c.id.clone();
        let next = Self::next_agent(c.agent);
        c.agent = next;
        self.pending_agent_overrides.insert(id.clone(), next);
        if c.is_scratch {
            self.scratch.set_agent(&id, next).ok();
        } else if let Some(registry) = self.registry.as_mut() {
            registry.update_agent(&id, next).ok();
        }
    }

    pub fn open_log_view(&mut self) {
        if let Some(c) = self.selected() {
            let (events, lines) = self.log_cache.get(&c.id).cloned().unwrap_or_default();
            let recap = self.recap.get(&c.id).cloned().unwrap_or_default();
            self.overlay = Overlay::LogView {
                container_id: c.id.clone(),
                events,
                lines,
                scroll: 0,
                recap,
                recap_visible: self.recap_visible,
            };
        }
    }

    /// Toggle whether the recap box is shown over the log text.
    pub fn toggle_recap(&mut self) {
        self.recap_visible = !self.recap_visible;
        if let Overlay::LogView {
            ref mut recap_visible,
            ..
        } = self.overlay
        {
            *recap_visible = self.recap_visible;
        }
    }

    /// Kick off (or refresh) the recap for the session shown in the log view.
    /// Shells out to `claude --print` off-thread; the result is delivered via
    /// `recap_rx` and picked up in `refresh()`.
    pub fn request_recap(&mut self) {
        let id = match &self.overlay {
            Overlay::LogView { container_id, .. } => container_id.clone(),
            _ => return,
        };
        let (events, lines) = self.log_cache.get(&id).cloned().unwrap_or_default();
        if events.is_empty() && lines.is_empty() {
            return;
        }
        self.recap.insert(id.clone(), Recap::Loading);
        // Requesting a recap always reveals the box, even if it was hidden.
        self.recap_visible = true;
        if let Overlay::LogView {
            ref mut recap,
            ref mut recap_visible,
            ..
        } = self.overlay
        {
            *recap = Recap::Loading;
            *recap_visible = true;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.recap_rx = Some(rx);
        tokio::spawn(async move {
            let result = crate::recap::generate(&events, &lines).await;
            tx.send((id, result)).ok();
        });
    }

    fn show_startup_greeting(&mut self) {
        if self.greeting.is_some() {
            return;
        }

        let name = crate::greeting::user_name();
        self.greeting = Some(Greeting::Ready(crate::greeting::fallback(&name)));
        self.greeting_name = Some(name);
        self.greeting_visible = true;
        self.greeting_deadline = Some(Instant::now() + GREETING_VISIBLE_FOR);
    }

    /// Start the optional model enhancement only after the first refresh has
    /// delivered containers, so the startup happy path remains independent of
    /// Claude's availability and latency.
    fn request_greeting_model(&mut self) {
        if self.greeting_rx.is_some() {
            return;
        }

        let Some(name) = self.greeting_name.take() else {
            return;
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.greeting_rx = Some(rx);
        tokio::spawn(async move {
            let result = crate::greeting::generate(&name).await;
            tx.send((name, result)).ok();
        });
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
            self.clamp_section_selection();
        }
    }

    fn undo_dismiss(&mut self) {
        let Some(container) = self.last_dismissed.take() else {
            return;
        };
        self.archive.restore_id(&container.id).ok();
        let section = if container.is_scratch {
            DashboardSection::Scratch
        } else {
            DashboardSection::Workspaces
        };
        let id = container.id.clone();
        self.containers.insert(0, container);
        self.select_id_in_section(&id, section);
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
            checked: Vec::new(),
        };
    }

    fn open_new_worktree(&mut self) {
        if self.registry.is_none() {
            return;
        }
        // Prefill from the selected container's registry checkouts so `n` on
        // a workspace proposes the same repo set; single-repo containers keep
        // the classic one-root prefill.
        let repo_roots = self
            .selected()
            .and_then(|c| {
                self.registry
                    .as_ref()
                    .and_then(|reg| reg.find_by_id(&c.id))
                    .map(|e| {
                        e.checkouts()
                            .iter()
                            .map(|ck| ck.repo_root.clone())
                            .collect()
                    })
            })
            .unwrap_or_default();
        self.overlay = Overlay::NewWorktree {
            name_buf: String::new(),
            agent: self.default_agent,
            repo_roots,
        };
    }

    fn open_new_scratch_chat(&mut self) {
        self.overlay = Overlay::NewScratchChat {
            title_buf: String::new(),
            agent: self.default_agent,
        };
    }

    fn cycle_scratch_agent(&mut self) {
        if let Overlay::NewScratchChat { ref mut agent, .. } = self.overlay {
            *agent = Self::next_agent(*agent);
        }
    }

    fn cycle_scratch_agent_back(&mut self) {
        if let Overlay::NewScratchChat { ref mut agent, .. } = self.overlay {
            *agent = Self::prev_agent(*agent);
        }
    }

    fn scratch_title_push(&mut self, c: char) {
        if let Overlay::NewScratchChat {
            ref mut title_buf, ..
        } = self.overlay
        {
            title_buf.push(c);
        }
    }

    fn scratch_title_backspace(&mut self) {
        if let Overlay::NewScratchChat {
            ref mut title_buf, ..
        } = self.overlay
        {
            title_buf.pop();
        }
    }

    fn confirm_new_scratch_chat(&mut self) -> Option<ScratchChat> {
        let (title, agent) = match &self.overlay {
            Overlay::NewScratchChat { title_buf, agent } => (title_buf.clone(), *agent),
            _ => return None,
        };
        self.scratch.create(title, agent).ok()
    }

    fn open_remove_confirm(&mut self) {
        let selected = match self.selected() {
            Some(c) => c.clone(),
            None => return,
        };
        if selected.is_scratch {
            self.overlay = Overlay::ScratchDeleteConfirm {
                container_id: selected.id,
            };
            return;
        }
        let entry = {
            let reg = match self.registry.as_ref() {
                Some(r) => r,
                None => return,
            };
            match reg.find_by_id(&selected.id) {
                Some(e) => e.clone(),
                None => return,
            }
        };
        self.overlay = Overlay::RemoveConfirm { entry };
    }

    fn cycle_agent(&mut self) {
        if let Overlay::NewWorktree { ref mut agent, .. } = self.overlay {
            *agent = Self::next_agent(*agent);
        }
    }

    fn cycle_agent_back(&mut self) {
        if let Overlay::NewWorktree { ref mut agent, .. } = self.overlay {
            *agent = Self::prev_agent(*agent);
        }
    }

    fn open_default_agent_picker(&mut self) {
        self.overlay = Overlay::DefaultAgent {
            agent: self.default_agent,
        };
    }

    fn cycle_default_agent(&mut self) {
        if let Overlay::DefaultAgent { ref mut agent } = self.overlay {
            *agent = Self::next_agent(*agent);
        }
    }

    fn cycle_default_agent_back(&mut self) {
        if let Overlay::DefaultAgent { ref mut agent } = self.overlay {
            *agent = Self::prev_agent(*agent);
        }
    }

    fn confirm_default_agent(&mut self) {
        if let Overlay::DefaultAgent { agent } = &self.overlay {
            let agent = *agent;
            if crate::config::Config::set_default_agent(agent).is_ok() {
                self.default_agent = agent;
            }
        }
        self.overlay = Overlay::None;
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
        let (name, agent_kind, repo_roots) = match &self.overlay {
            Overlay::NewWorktree {
                name_buf,
                agent,
                repo_roots,
            } => {
                let name = if name_buf.is_empty() {
                    None
                } else {
                    Some(name_buf.clone())
                };
                (name, *agent, repo_roots.clone())
            }
            _ => return None,
        };
        self.overlay = Overlay::None;
        if let Some(registry) = self.registry.as_mut() {
            let opts = CreateOptions {
                name,
                agent: agent_kind,
                repo_roots,
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

    fn confirm_scratch_delete(&mut self) {
        let id = match &self.overlay {
            Overlay::ScratchDeleteConfirm { container_id } => container_id.clone(),
            _ => return,
        };
        self.overlay = Overlay::None;
        self.scratch.delete(&id).ok();
        self.archive.restore_id(&id).ok();
        self.last_sent.remove(&id);
        self.log_cache.remove(&id);
        self.recap.remove(&id);
        if let Some(index) = self
            .containers
            .iter()
            .position(|container| container.id == id)
        {
            self.containers.remove(index);
            self.clamp_section_selection();
        }
    }
}

pub type AdapterMap = HashMap<AgentKind, Arc<dyn AgentAdapter>>;

struct RefreshResult {
    containers: Vec<Container>,
    pr_cache: HashMap<String, (Option<PrProbe>, Instant)>,
    branch_cache: HashMap<String, (String, Instant)>,
    selected_log: Option<(String, Vec<vigil_core::LogEvent>, Vec<String>)>,
}

#[derive(Debug, Clone)]
struct PrProbe {
    status: PrStatus,
    url: Option<String>,
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
    app.show_startup_greeting();
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
                            KeyCode::Char('w') => app.focus_workspaces(),
                            KeyCode::Char('s') => app.focus_scratch(),
                            KeyCode::Tab => app.cycle_selected_agent(),
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
                            KeyCode::Char('n') => app.open_new_worktree(),
                            KeyCode::Char('c') => app.open_new_scratch_chat(),
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
                            KeyCode::Char('o') => {
                                if let Some(c) = app.selected() {
                                    let path = c.worktree_path.clone();
                                    open_terminal(terminal, &path)?;
                                    terminal.clear()?;
                                }
                            }
                            KeyCode::Char('R') => app.open_remove_confirm(),
                            KeyCode::Char('S') => app.open_default_agent_picker(),
                            KeyCode::Enter => {
                                if let Some(c) = app.selected() {
                                    if let Some((session_id, path, agent)) =
                                        launch_target(&app.containers, &c.id)
                                    {
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
                    } else if matches!(app.overlay, Overlay::NewScratchChat { .. }) {
                        match key.code {
                            KeyCode::Esc => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Tab => {
                                app.cycle_scratch_agent();
                            }
                            KeyCode::BackTab => {
                                app.cycle_scratch_agent_back();
                            }
                            KeyCode::Backspace => {
                                app.scratch_title_backspace();
                            }
                            KeyCode::Enter => {
                                if let Some(chat) = app.confirm_new_scratch_chat() {
                                    let id = chat.id.clone();
                                    app.containers.push(scratch_container(&chat));
                                    app.focus_scratch();
                                    let scratch_position = app
                                        .scratch_indices()
                                        .iter()
                                        .position(|index| app.containers[*index].id == id);
                                    app.scratch_table_state.select(scratch_position);
                                    app.overlay = Overlay::SendMessage {
                                        input: Vec::new(),
                                        container_id: Some(id),
                                        return_to_log: false,
                                    };
                                }
                            }
                            KeyCode::Char(c) => {
                                app.scratch_title_push(c);
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::LogView { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('q') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Enter => {
                                let target =
                                    if let Overlay::LogView { container_id, .. } = &app.overlay {
                                        launch_target(&app.containers, container_id)
                                    } else {
                                        None
                                    };
                                if let Some((session_id, path, agent)) = target {
                                    app.overlay = Overlay::None;
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
                            KeyCode::Char('r') => {
                                app.request_recap();
                            }
                            KeyCode::Char('R') => {
                                app.toggle_recap();
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
                    } else if matches!(app.overlay, Overlay::ScratchDeleteConfirm { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.confirm_scratch_delete();
                            }
                            _ => {}
                        }
                    } else if matches!(app.overlay, Overlay::DefaultAgent { .. }) {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                app.overlay = Overlay::None;
                            }
                            KeyCode::Tab | KeyCode::Down | KeyCode::Right | KeyCode::Char('j') => {
                                app.cycle_default_agent();
                            }
                            KeyCode::BackTab | KeyCode::Up | KeyCode::Left | KeyCode::Char('k') => {
                                app.cycle_default_agent_back();
                            }
                            KeyCode::Enter => {
                                app.confirm_default_agent();
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
                            KeyCode::Char(' ') => {
                                // Toggle the highlighted repo in the multi-select set.
                                if let Overlay::ProjectPicker {
                                    ref all_repos,
                                    ref query,
                                    selected_idx,
                                    ref mut checked,
                                    ..
                                } = app.overlay
                                {
                                    if let Some(repo) =
                                        picker_filtered_at(all_repos, query, selected_idx)
                                    {
                                        match checked.iter().position(|p| p == &repo) {
                                            Some(i) => {
                                                checked.remove(i);
                                            }
                                            None => checked.push(repo),
                                        }
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                // Checked repos win; otherwise fall back to the
                                // highlighted repo (classic single-select).
                                let repo_roots = if let Overlay::ProjectPicker {
                                    ref all_repos,
                                    ref query,
                                    selected_idx,
                                    ref checked,
                                    ..
                                } = app.overlay
                                {
                                    if checked.is_empty() {
                                        picker_filtered_at(all_repos, query, selected_idx)
                                            .map(|p| vec![p])
                                            .unwrap_or_default()
                                    } else {
                                        checked.clone()
                                    }
                                } else {
                                    vec![]
                                };
                                if !repo_roots.is_empty() {
                                    app.scan_rx = None;
                                    app.overlay = Overlay::NewWorktree {
                                        name_buf: String::new(),
                                        agent: app.default_agent,
                                        repo_roots,
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
                                    let target = match container_id.as_ref() {
                                        Some(id) => app.containers.iter().find(|c| &c.id == id),
                                        None => app.selected(),
                                    };
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
    let recap = app.recap.get(container_id).cloned().unwrap_or_default();
    Overlay::LogView {
        container_id: container_id.to_string(),
        events,
        lines,
        scroll: 0,
        recap,
        recap_visible: app.recap_visible,
    }
}

async fn refresh(app: &mut App, adapters: &AdapterMap) {
    app.registry = Registry::load().ok();

    let selected_id = app.selected().map(|c| c.id.clone());
    let mut refresh_completed = false;

    if let Some(rx) = app.refresh_rx.as_mut() {
        match rx.try_recv() {
            Ok(result) => {
                app.refresh_rx = None;
                refresh_completed = true;
                app.pr_cache = result.pr_cache;
                app.branch_cache = result.branch_cache;
                if let Some((id, events, lines)) = result.selected_log {
                    app.log_cache.insert(id, (events, lines));
                }
                app.containers = apply_pending_agent_overrides(
                    result.containers,
                    &mut app.pending_agent_overrides,
                );
                ensure_scratch_rows(app);
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                app.refresh_rx = None;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
    }

    if refresh_completed {
        app.request_greeting_model();
    }

    // Pick up a completed recap, if any, and cache it by container id.
    if let Some(rx) = app.recap_rx.as_mut() {
        match rx.try_recv() {
            Ok((id, result)) => {
                app.recap_rx = None;
                let recap = match result {
                    Ok(text) => Recap::Ready(text),
                    Err(err) => Recap::Error(err),
                };
                app.recap.insert(id, recap);
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                app.recap_rx = None;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
    }

    // Pick up the optional model-enhanced startup greeting. The deterministic
    // fallback remains visible if this request fails or takes too long.
    if let Some(rx) = app.greeting_rx.as_mut() {
        match rx.try_recv() {
            Ok((name, result)) => {
                app.greeting_rx = None;
                if app.greeting_visible {
                    if let Ok(text) = result {
                        app.greeting = Some(Greeting::Ready(text));
                    } else {
                        app.greeting_name = Some(name);
                    }
                } else if result.is_err() {
                    app.greeting_name = Some(name);
                }
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                app.greeting_rx = None;
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
    }

    if app
        .greeting_deadline
        .map(|deadline| Instant::now() >= deadline)
        .unwrap_or(false)
    {
        app.greeting_visible = false;
        app.greeting_deadline = None;
    }

    // Reconcile cached rows against the current registry/archive immediately. The
    // expensive part (adapter probing, PR lookups, logs) happens in the background;
    // this cheap diff keeps deleted/dismissed rows from hanging around while that
    // work is in flight.
    let mut live_ids: std::collections::HashSet<String> = match &app.registry {
        Some(reg) => reg
            .entries()
            .iter()
            .filter(|e| !app.archive.is_dismissed_id(&e.id))
            .map(|e| e.id.clone())
            .collect(),
        None => std::collections::HashSet::new(),
    };
    live_ids.extend(
        app.scratch
            .chats()
            .iter()
            .filter(|chat| !app.archive.is_dismissed_id(&chat.id))
            .map(|chat| chat.id.clone()),
    );
    app.containers.retain(|c| live_ids.contains(&c.id));
    sort_containers(&mut app.containers);

    // If we're waiting to auto-select a freshly-created worktree, check if it
    // has now appeared in the container list (i.e. background probe completed).
    let pending_id = app.pending_select_id.take().or_else(|| {
        let pending_path_id = app
            .pending_select_path
            .as_ref()
            .and_then(|path| app.containers.iter().find(|c| &c.worktree_path == path))
            .map(|c| c.id.clone());
        app.pending_select_path.take();
        pending_path_id
    });
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
        recap,
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
        if let Some(cached_recap) = app.recap.get(container_id) {
            *recap = cached_recap.clone();
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
        let scratch_chats = app
            .scratch
            .chats()
            .iter()
            .filter(|chat| !app.archive.is_dismissed_id(&chat.id))
            .cloned()
            .collect();
        let adapters = adapters.clone();
        let pr_cache = app.pr_cache.clone();
        let branch_cache = app.branch_cache.clone();
        let last_sent = app.last_sent.clone();
        let selected_log_info = app
            .selected()
            .map(|c| (c.id.clone(), c.worktree_path.clone(), c.agent));
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.refresh_rx = Some(rx);
        tokio::spawn(async move {
            let result = build_refresh_result(
                entries,
                scratch_chats,
                adapters,
                pr_cache,
                branch_cache,
                last_sent,
                selected_log_info,
            )
            .await;
            tx.send(result).ok();
        });
    }

    app.last_refresh = Instant::now();
}

async fn build_refresh_result(
    entries: Vec<WorktreeEntry>,
    scratch_chats: Vec<ScratchChat>,
    adapters: AdapterMap,
    mut pr_cache: HashMap<String, (Option<PrProbe>, Instant)>,
    mut branch_cache: HashMap<String, (String, Instant)>,
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

    let mut scratch_handles = Vec::with_capacity(scratch_chats.len());
    for chat in &scratch_chats {
        let path = chat.workdir.clone();
        let adapter = adapters
            .get(&chat.agent)
            .or_else(|| adapters.values().next())
            .map(Arc::clone);
        scratch_handles.push(tokio::spawn(async move {
            match adapter {
                Some(a) => a.probe(&path).await,
                None => ProbeResult::no_session(),
            }
        }));
    }

    // Re-read each checkout's live git branch, but only when its cache entry has
    // aged past BRANCH_REFRESH. The branch recorded in the registry is only set
    // at creation time, so a later `git switch` would otherwise leave a stale
    // name everywhere (main view, PR lookup); polling on a slower cadence than
    // the tick keeps that fresh without a `git` fork per worktree every second.
    // Probes run per checkout (keyed id::repo_root) — a workspace's parent dir
    // is not a git repo, and each contained repo has its own branch.
    let mut branch_handles = Vec::new();
    for entry in &entries {
        for checkout in entry.checkouts() {
            let key = probe_cache_key(&entry.id, &checkout.repo_root);
            let stale = branch_cache
                .get(&key)
                .is_none_or(|(_, t)| t.elapsed() >= BRANCH_REFRESH);
            if stale {
                let path = checkout.worktree_path.clone();
                let fallback = checkout.branch.clone();
                branch_handles.push((
                    key,
                    fallback,
                    tokio::spawn(async move { current_branch(&path).await }),
                ));
            }
        }
    }
    for (key, fallback, handle) in branch_handles {
        let branch = handle.await.unwrap_or(None).unwrap_or(fallback);
        branch_cache.insert(key, (branch, Instant::now()));
    }
    let live_branch = |entry: &WorktreeEntry, checkout: &atelier_worktree::RepoCheckout| {
        branch_cache
            .get(&probe_cache_key(&entry.id, &checkout.repo_root))
            .map(|(b, _)| b.clone())
            .unwrap_or_else(|| checkout.branch.clone())
    };

    let mut pr_handles: Vec<(String, _)> = Vec::new();
    for entry in &entries {
        for checkout in entry.checkouts() {
            let key = probe_cache_key(&entry.id, &checkout.repo_root);
            let stale = pr_cache
                .get(&key)
                .is_none_or(|(_, t)| t.elapsed() >= PR_REFRESH);
            if stale {
                let repo_root = checkout.repo_root.clone();
                let branch = live_branch(entry, &checkout);
                pr_handles.push((
                    key,
                    tokio::spawn(async move { probe_pr(&repo_root, &branch).await }),
                ));
            }
        }
    }
    for (key, handle) in pr_handles {
        let status = handle.await.unwrap_or(None);
        pr_cache.insert(key, (status, Instant::now()));
    }

    let cwd_procs = cwd_snapshot().await;

    let mut containers = Vec::with_capacity(entries.len());
    for (entry, handle) in entries.iter().zip(handles) {
        let probe: ProbeResult = handle.await.unwrap_or_else(|_| ProbeResult::no_session());
        let checkouts = entry.checkouts();
        let repo_prs: Vec<Option<PrProbe>> = checkouts
            .iter()
            .map(|ck| {
                pr_cache
                    .get(&probe_cache_key(&entry.id, &ck.repo_root))
                    .and_then(|(probe, _)| probe.clone())
            })
            .collect();
        let pr_status = aggregate_pr_status(
            &repo_prs
                .iter()
                .map(|probe| probe.as_ref().map(|probe| probe.status.clone()))
                .collect::<Vec<_>>(),
        );
        let pr_url = pr_status.as_ref().and_then(|status| {
            repo_prs.iter().find_map(|probe| {
                probe.as_ref().and_then(|probe| {
                    if &probe.status == status {
                        probe.url.clone()
                    } else {
                        None
                    }
                })
            })
        });
        let repo_statuses: Vec<RepoStatus> = checkouts
            .iter()
            .zip(repo_prs.iter())
            .map(|(ck, probe)| RepoStatus {
                repo_root: ck.repo_root.clone(),
                worktree_path: ck.worktree_path.clone(),
                branch: live_branch(entry, ck),
                pr_status: probe.as_ref().map(|probe| probe.status.clone()),
            })
            .collect();
        let branch = repo_statuses[0].branch.clone();
        let background_processes = collect_background_processes(&cwd_procs, &entry.worktree_path);
        containers.push(Container {
            id: entry.id.clone(),
            display_name: None,
            worktree_path: entry.worktree_path.clone(),
            repo_root: entry.repo_root.clone(),
            agent: entry.agent,
            branch,
            created_at: entry.created_at,
            state: probe.state,
            session_id: probe.session_id,
            last_activity: probe.last_activity,
            last_user_message: last_sent
                .get(&entry.id)
                .cloned()
                .or(probe.last_user_message),
            pr_status,
            pr_url,
            background_processes,
            // Empty for single-repo entries so classic rendering paths stay
            // untouched.
            repos: if entry.repos.is_empty() {
                vec![]
            } else {
                repo_statuses
            },
            is_scratch: false,
        });
    }
    for (chat, handle) in scratch_chats.iter().zip(scratch_handles) {
        let probe = handle.await.unwrap_or_else(|_| ProbeResult::no_session());
        let mut container = scratch_container(chat);
        container.state = probe.state;
        container.session_id = probe.session_id;
        container.last_activity = probe.last_activity;
        container.last_user_message = last_sent.get(&chat.id).cloned().or(probe.last_user_message);
        container.background_processes = collect_background_processes(&cwd_procs, &chat.workdir);
        containers.push(container);
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
        branch_cache,
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

fn ensure_scratch_rows(app: &mut App) {
    let active_chats: Vec<ScratchChat> = app
        .scratch
        .chats()
        .iter()
        .filter(|chat| !app.archive.is_dismissed_id(&chat.id))
        .cloned()
        .collect();
    for chat in active_chats {
        if !app
            .containers
            .iter()
            .any(|container| container.id == chat.id)
        {
            app.containers.push(scratch_container(&chat));
        }
    }
    sort_containers(&mut app.containers);
}

fn restore_selection(app: &mut App, selected_id: Option<String>) {
    if let Some(id) = selected_id {
        if let Some(container) = app.containers.iter().find(|container| container.id == id) {
            let section = if container.is_scratch {
                DashboardSection::Scratch
            } else {
                DashboardSection::Workspaces
            };
            app.select_id_in_section(&id, section);
            return;
        }
    }

    if app.section == DashboardSection::Workspaces
        && app.workspace_indices().is_empty()
        && !app.scratch_indices().is_empty()
    {
        app.focus_scratch();
    } else if app.section == DashboardSection::Scratch
        && app.scratch_indices().is_empty()
        && !app.workspace_indices().is_empty()
    {
        app.focus_workspaces();
    }
    app.clamp_section_selection();
}

fn apply_pending_agent_overrides(
    mut containers: Vec<Container>,
    pending: &mut HashMap<String, AgentKind>,
) -> Vec<Container> {
    for container in &mut containers {
        let Some(expected_agent) = pending.get(&container.id).copied() else {
            continue;
        };

        if container.agent == expected_agent {
            pending.remove(&container.id);
        } else {
            container.agent = expected_agent;
        }
    }
    containers
}

/// Commands that are part of the agent/shell harness running inside a worktree,
/// and therefore not interesting as "background tasks" the user spawned.
const BG_PROCESS_EXCLUDE: &[&str] = &[
    "claude",
    "pi",
    "droid",
    "codex",
    "opencode",
    "factory",
    "vigil",
    "vigil-tui",
    "vigil-cli",
    "atelier",
    "atelier-cli",
    "bash",
    "zsh",
    "fish",
    "sh",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "tmux",
    "tmux: server",
    "screen",
    "nvim",
    "vim",
    "nano",
    "git",
    "ssh",
    "ssh-agent",
    "less",
    "more",
    "man",
    "lsof",
    "ps",
    "sourcekit-lsp",
];

fn is_bg_process_candidate(command: &str) -> bool {
    !BG_PROCESS_EXCLUDE.contains(&command)
}

fn collect_background_processes(
    snapshot: &[vigil_core::process::CwdProcess],
    worktree_path: &Path,
) -> Vec<BackgroundProcess> {
    snapshot
        .iter()
        .filter(|p| p.cwd.starts_with(worktree_path))
        .filter(|p| is_bg_process_candidate(&p.command))
        .map(|p| BackgroundProcess {
            pid: p.pid,
            command: p.command.clone(),
        })
        .collect()
}

/// Query `gh pr view` to get PR status for a branch. Returns None if gh is unavailable.
/// Read the worktree's current git branch via `git rev-parse --abbrev-ref HEAD`.
///
/// Returns `None` on error or when HEAD is detached (the command prints
/// `"HEAD"`), so callers can fall back to the branch recorded at creation time.
async fn current_branch(worktree_path: &std::path::Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

async fn probe_pr(repo_root: &std::path::Path, branch: &str) -> Option<PrProbe> {
    let output = tokio::process::Command::new("gh")
        .args(["pr", "view", branch, "--json", "url,state,mergeStateStatus"])
        .current_dir(repo_root)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return Some(PrProbe {
            status: PrStatus::NoPr,
            url: None,
        });
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let status = match json["state"].as_str()? {
        "MERGED" => PrStatus::Merged,
        "OPEN" => {
            if json["mergeStateStatus"].as_str() == Some("CLEAN") {
                PrStatus::ReadyToMerge
            } else {
                PrStatus::InProgress
            }
        }
        _ => PrStatus::NoPr,
    };
    let url = match status {
        PrStatus::NoPr => None,
        _ => json["url"].as_str().map(str::to_owned),
    };
    Some(PrProbe { status, url })
}

/// The repo at `idx` in the ProjectPicker's filtered view, if any.
fn picker_filtered_at(all_repos: &[PathBuf], query: &str, idx: usize) -> Option<PathBuf> {
    let q = query.to_lowercase();
    all_repos
        .iter()
        .filter(|p| p.display().to_string().to_lowercase().contains(&q))
        .nth(idx)
        .cloned()
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

/// Open a shell in `dir`, suspending the TUI until the shell exits.
fn open_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    dir: &std::path::Path,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    std::process::Command::new(&shell)
        .current_dir(dir)
        .status()
        .ok();

    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableBracketedPaste
    )?;

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use vigil_core::SessionState;

    fn container(agent: AgentKind) -> Container {
        Container {
            id: "firm-hilbert".to_string(),
            display_name: None,
            worktree_path: PathBuf::from("/tmp/firm-hilbert"),
            repo_root: PathBuf::from("/tmp/repo"),
            agent,
            branch: "firm-hilbert".to_string(),
            created_at: Utc::now(),
            state: SessionState::NoSession,
            session_id: None,
            last_activity: None,
            last_user_message: None,
            pr_status: None,
            pr_url: None,
            background_processes: Vec::new(),
            repos: Vec::new(),
            is_scratch: false,
        }
    }

    #[test]
    fn stale_refresh_keeps_the_pending_agent_switch() {
        let mut pending = HashMap::from([("firm-hilbert".to_string(), AgentKind::Codex)]);

        let containers =
            apply_pending_agent_overrides(vec![container(AgentKind::ClaudeCode)], &mut pending);

        assert_eq!(containers[0].agent, AgentKind::Codex);
        assert_eq!(pending.get("firm-hilbert"), Some(&AgentKind::Codex));
    }

    #[test]
    fn refresh_clears_the_override_once_persisted_agent_arrives() {
        let mut pending = HashMap::from([("firm-hilbert".to_string(), AgentKind::Codex)]);

        let containers =
            apply_pending_agent_overrides(vec![container(AgentKind::Codex)], &mut pending);

        assert_eq!(containers[0].agent, AgentKind::Codex);
        assert!(pending.is_empty());
    }

    #[test]
    fn launch_target_resolves_the_requested_container() {
        let containers = vec![container(AgentKind::Codex)];

        let (session_id, path, agent) = launch_target(&containers, "firm-hilbert").unwrap();

        assert_eq!(session_id, None);
        assert_eq!(path, PathBuf::from("/tmp/firm-hilbert"));
        assert_eq!(agent, AgentKind::Codex);
        assert!(launch_target(&containers, "missing").is_none());
    }
}
