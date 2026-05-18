//! Pi adapter for Vigil.
//!
//! State classification is process-based: we check whether a `pi` process
//! has any files open inside the worktree directory (`lsof +D`). If yes the
//! session is Running; if not, we use the most-recently-modified file's mtime
//! to decide between Idle (modified recently, no process) and Done (quiescent).

use std::path::Path;
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, ProbeResult, SessionId, SessionState};

pub struct PiAdapter;

#[async_trait::async_trait]
impl AgentAdapter for PiAdapter {
    fn kind(&self) -> AgentKind { AgentKind::Pi }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let state = classify(dir).await;
        ProbeResult {
            state,
            session_id: None,
            last_activity: newest_mtime(dir)
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| {
                    let secs = d.as_secs() as i64;
                    chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default()
                }),
            last_user_message: None,
        }
    }

    fn attach_command(&self, _session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.current_dir(dir);
        cmd
    }
}

/// Returns Running if a `pi` process has files open under `dir`, otherwise
/// Idle (recently written) or Done (quiescent / no activity).
async fn classify(dir: &Path) -> SessionState {
    match pi_is_active(dir).await {
        Some(true) => SessionState::Running,
        Some(false) => {
            let secs_idle = newest_mtime(dir)
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(f64::MAX);

            if secs_idle < 30.0 {
                SessionState::Idle
            } else {
                SessionState::Done
            }
        }
        None => SessionState::NoSession,
    }
}

/// Use `lsof +D <dir>` filtered to processes named `pi`.
/// Returns `Some(true)` if active, `Some(false)` if confirmed inactive, `None` on failure.
async fn pi_is_active(dir: &Path) -> Option<bool> {
    let output = tokio::process::Command::new("lsof")
        .args(["+D"])
        .arg(dir)
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let active = stdout.lines().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("pi ") || lower.contains("/pi ")
    });

    tracing::debug!("pi_is_active({}) = {active}", dir.display());
    Some(active)
}

/// Walk the worktree directory one level deep and return the newest file mtime.
fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| {
        e.metadata().ok()?.modified().ok()
    }).max()
}
