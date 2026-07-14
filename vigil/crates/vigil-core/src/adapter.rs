use crate::{wrap_agent_harness_command, AgentKind, LogEvent, SessionId, SessionState, VigilError};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::process::{Command, Stdio};

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

    /// Build the agent-native command to attach to an existing session.
    /// Callers should use `attach_command()` so harness wrapping is applied uniformly.
    fn raw_attach_command(&self, session_id: &SessionId, dir: &Path) -> Command;

    /// Build the agent-native command to launch a fresh session.
    /// Callers should use `launch_command()` so harness wrapping is applied uniformly.
    fn raw_launch_command(&self, dir: &Path) -> Command;

    /// Build a command to attach to an existing session found by a prior `probe()`.
    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> Command {
        wrap_agent_harness_command(self.kind(), dir, self.raw_attach_command(session_id, dir))
    }

    /// Build a command to launch a fresh agent session in `dir`.
    fn launch_command(&self, dir: &Path) -> Command {
        wrap_agent_harness_command(self.kind(), dir, self.raw_launch_command(dir))
    }

    /// Return the most recent log lines for the session in `dir`, formatted for display.
    /// Each string is one line. Returns empty if the agent has no log to show.
    async fn recent_log(&self, _dir: &Path) -> Vec<String> {
        vec![]
    }

    /// Return structured log events for the session in `dir`, grouped into turns.
    /// Adapters with JSONL session files override this for the timeline log-view design.
    async fn recent_log_events(&self, _dir: &Path) -> Vec<LogEvent> {
        vec![]
    }

    /// Send a short message to the agent session running in `dir`.
    /// The default implementation builds a command, applies harness wrapping, and spawns it.
    async fn send_message(
        &self,
        dir: &Path,
        session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        let cmd = self.raw_send_message_command(dir, session_id, msg)?;
        spawn_wrapped_background_command(self.kind(), dir, cmd).await
    }

    /// Start a brand-new agent session in `dir` with an initial message, running in the
    /// background (fire-and-forget). Used when there is no existing session to resume.
    /// The default implementation builds a command, applies harness wrapping, and spawns it.
    async fn start_with_message(&self, dir: &Path, msg: &str) -> Result<(), VigilError> {
        let cmd = self.raw_start_with_message_command(dir, msg)?;
        spawn_wrapped_background_command(self.kind(), dir, cmd).await
    }

    /// Build the agent-native command for `send_message`.
    fn raw_send_message_command(
        &self,
        _dir: &Path,
        _session_id: &SessionId,
        _msg: &str,
    ) -> Result<Command, VigilError> {
        Err(VigilError::NotSupported(
            "send_message not implemented".into(),
        ))
    }

    /// Build the agent-native command for `start_with_message`.
    fn raw_start_with_message_command(
        &self,
        _dir: &Path,
        _msg: &str,
    ) -> Result<Command, VigilError> {
        Err(VigilError::NotSupported(
            "start_with_message not implemented".into(),
        ))
    }
}

async fn spawn_wrapped_background_command(
    agent: AgentKind,
    dir: &Path,
    command: Command,
) -> Result<(), VigilError> {
    let mut cmd = wrap_agent_harness_command(agent, dir, command);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    tokio::process::Command::from(cmd).spawn().map_err(|e| {
        VigilError::ProcessProbe(format!("{} spawn failed: {e}", agent.config_key()))
    })?;
    Ok(())
}
