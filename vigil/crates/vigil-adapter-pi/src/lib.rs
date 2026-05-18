//! Pi adapter for Vigil (stub — not yet implemented).

use std::path::Path;
use vigil_core::{AgentAdapter, AgentKind, ProbeResult, SessionId};

pub struct PiAdapter;

#[async_trait::async_trait]
impl AgentAdapter for PiAdapter {
    fn kind(&self) -> AgentKind { AgentKind::Pi }

    async fn probe(&self, _dir: &Path) -> ProbeResult {
        ProbeResult::no_session()
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.arg("--session").arg(&session_id.0).current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.current_dir(dir);
        cmd
    }
}
