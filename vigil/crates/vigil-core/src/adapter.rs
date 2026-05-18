use std::path::Path;
use chrono::{DateTime, Utc};
use crate::{AgentKind, LogEvent, SessionId, SessionState};

/// Result of probing an agent for a given container directory.
pub struct ProbeResult {
    pub state: SessionState,
    /// The session id, if an agent session was found. Required for attach_command.
    pub session_id: Option<SessionId>,
    pub last_activity: Option<DateTime<Utc>>,
    pub last_user_message: Option<String>,
}

impl ProbeResult {
    pub fn no_session() -> Self {
        Self {
            state: SessionState::NoSession,
            session_id: None,
            last_activity: None,
            last_user_message: None,
        }
    }
}

#[async_trait::async_trait]
pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Probe whether an agent session is running (or recently ran) in `dir`.
    async fn probe(&self, dir: &Path) -> ProbeResult;

    /// Build a command to attach to an existing session found by a prior `probe()`.
    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command;

    /// Build a command to launch a fresh agent session in `dir`.
    fn launch_command(&self, dir: &Path) -> std::process::Command;

    /// Return the most recent log lines for the session in `dir`, formatted for display.
    /// Each string is one line. Returns empty if the agent has no log to show.
    async fn recent_log(&self, _dir: &Path) -> Vec<String> { vec![] }

    /// Return structured log events for the session in `dir`, grouped into turns.
    /// Adapters with JSONL session files override this for the timeline log-view design.
    async fn recent_log_events(&self, _dir: &Path) -> Vec<LogEvent> { vec![] }

    /// Send a short message to the agent session running in `dir`.
    /// The default implementation returns `NotSupported`; adapters that support it override this.
    async fn send_message(
        &self,
        _dir: &Path,
        _session_id: &SessionId,
        _msg: &str,
    ) -> Result<(), crate::VigilError> {
        Err(crate::VigilError::NotSupported("send_message not implemented".into()))
    }
}
