use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    Pi,
    OpenCode,
}

impl AgentKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::Pi => "Pi",
            AgentKind::OpenCode => "OpenCode",
        }
    }

    /// Whether this agent is fully implemented and should be shown in the picker.
    pub fn available(&self) -> bool {
        match self {
            AgentKind::ClaudeCode => true,
            AgentKind::Codex => false,
            AgentKind::Pi => true,
            AgentKind::OpenCode => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// No agent session has ever run (or been found) in this container.
    NoSession,
    Running,
    AwaitingInput {
        reason: Option<String>,
    },
    Idle,
    Done,
    Error {
        message: String,
    },
    Unknown,
}

impl SessionState {
    pub fn needs_attention(&self) -> bool {
        matches!(self, SessionState::AwaitingInput { .. })
    }
}

/// PR / branch status as seen by the GitHub CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrStatus {
    /// No open or merged PR found for this branch.
    NoPr,
    /// PR is open but not yet ready to merge (CI running, changes requested, etc.)
    InProgress,
    /// PR is open and the merge state is clean (CI green, approvals met).
    ReadyToMerge,
    /// PR has been merged.
    Merged,
}

/// The primary entity: a registered worktree that may or may not have an active agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    /// Registry id — the worktree branch name (e.g. "firm-hilbert").
    pub id: String,
    pub worktree_path: PathBuf,
    pub repo_root: PathBuf,
    pub agent: AgentKind,
    pub branch: String,
    pub created_at: DateTime<Utc>,
    /// State determined by probing the agent adapter.
    pub state: SessionState,
    /// Session id from the adapter, needed to build an attach command.
    pub session_id: Option<SessionId>,
    pub last_activity: Option<DateTime<Utc>>,
    /// Most recent user message visible to the agent, if any.
    pub last_user_message: Option<String>,
    /// PR status for this container's branch, if known.
    pub pr_status: Option<PrStatus>,
}
