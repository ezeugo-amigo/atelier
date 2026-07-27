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
    Droid,
}

impl AgentKind {
    /// All known agent kinds in display order. Keep this in sync with the enum.
    /// `available()` and `display_name()` below use exhaustive matches, so the
    /// compiler catches missing variants there — add new variants to all three.
    pub fn all() -> &'static [AgentKind] {
        &[
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::Pi,
            AgentKind::OpenCode,
            AgentKind::Droid,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::Pi => "Pi",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Droid => "Droid",
        }
    }

    pub fn config_key(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::Pi => "pi",
            AgentKind::OpenCode => "opencode",
            AgentKind::Droid => "droid",
        }
    }

    pub fn from_config_key(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claudecode" => Some(AgentKind::ClaudeCode),
            "codex" => Some(AgentKind::Codex),
            "pi" => Some(AgentKind::Pi),
            "opencode" | "open-code" => Some(AgentKind::OpenCode),
            "droid" => Some(AgentKind::Droid),
            _ => None,
        }
    }

    /// Whether this agent is fully implemented and should be shown in the picker.
    pub fn available(&self) -> bool {
        match self {
            AgentKind::ClaudeCode => true,
            AgentKind::Codex => true,
            AgentKind::Pi => true,
            AgentKind::OpenCode => true,
            AgentKind::Droid => true,
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

/// Per-repo state inside a multi-repo workspace container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStatus {
    pub repo_root: PathBuf,
    /// The repo's worktree checkout inside the workspace dir.
    pub worktree_path: PathBuf,
    /// Live branch of that checkout.
    pub branch: String,
    pub pr_status: Option<PrStatus>,
}

/// Combine per-repo PR statuses into one container-level status: the "least
/// done" PR wins (InProgress < ReadyToMerge < Merged), so the dot reflects the
/// repo that still needs work. NoPr counts only when no repo has a PR at all.
pub fn aggregate_pr_status(statuses: &[Option<PrStatus>]) -> Option<PrStatus> {
    fn rank(s: &PrStatus) -> u8 {
        match s {
            PrStatus::InProgress => 0,
            PrStatus::ReadyToMerge => 1,
            PrStatus::Merged => 2,
            PrStatus::NoPr => 3,
        }
    }
    let known: Vec<&PrStatus> = statuses.iter().flatten().collect();
    let min = known.iter().min_by_key(|s| rank(s))?;
    Some((*min).clone())
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
    /// PR URL for this container's branch, if one was found.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Non-agent processes whose working directory lives under `worktree_path`
    /// (e.g. dev servers spawned by the agent). Empty when nothing is running.
    #[serde(default)]
    pub background_processes: Vec<BackgroundProcess>,
    /// Per-repo state for multi-repo workspace containers. Empty for classic
    /// single-repo containers; when populated, `repo_root`/`branch` mirror the
    /// first repo and `pr_status` holds the aggregate.
    #[serde(default)]
    pub repos: Vec<RepoStatus>,
}

/// A user-spawned process living inside a container (e.g. a dev server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundProcess {
    pub pid: u32,
    pub command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_none_when_no_repo_probed() {
        assert_eq!(aggregate_pr_status(&[None, None]), None);
        assert_eq!(aggregate_pr_status(&[]), None);
    }

    #[test]
    fn aggregate_least_done_wins() {
        assert_eq!(
            aggregate_pr_status(&[Some(PrStatus::Merged), Some(PrStatus::InProgress)]),
            Some(PrStatus::InProgress)
        );
        assert_eq!(
            aggregate_pr_status(&[Some(PrStatus::Merged), Some(PrStatus::ReadyToMerge)]),
            Some(PrStatus::ReadyToMerge)
        );
        assert_eq!(
            aggregate_pr_status(&[Some(PrStatus::Merged), Some(PrStatus::Merged)]),
            Some(PrStatus::Merged)
        );
    }

    #[test]
    fn aggregate_no_pr_only_when_no_repo_has_one() {
        assert_eq!(
            aggregate_pr_status(&[Some(PrStatus::NoPr), Some(PrStatus::Merged)]),
            Some(PrStatus::Merged)
        );
        assert_eq!(
            aggregate_pr_status(&[Some(PrStatus::NoPr), None]),
            Some(PrStatus::NoPr)
        );
    }
}
