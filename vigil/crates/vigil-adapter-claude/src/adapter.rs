use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, FsSignals, ProbeResult, SessionId, VigilError};

use crate::{classifier, history, log_parser, process};

#[derive(Clone)]
pub struct ClaudeCodeAdapter {
    debug_dir: PathBuf,
    history_path: PathBuf,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Result<Self, VigilError> {
        let base = directories::BaseDirs::new().ok_or_else(|| {
            VigilError::ProcessProbe("cannot determine home directory".into())
        })?;
        let claude_dir = base.home_dir().join(".claude");
        Ok(Self {
            debug_dir: claude_dir.join("debug"),
            history_path: claude_dir.join("history.jsonl"),
        })
    }

    pub fn with_paths(debug_dir: PathBuf, history_path: PathBuf) -> Self {
        Self { debug_dir, history_path }
    }

    fn session_path(&self, id: &SessionId) -> PathBuf {
        self.debug_dir.join(format!("{}.txt", id.0))
    }
}

#[async_trait::async_trait]
impl AgentAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> AgentKind { AgentKind::ClaudeCode }

    /// Probe the container at `dir`: find the most recent Claude session for that directory,
    /// read its debug log, and classify the state.
    async fn probe(&self, dir: &Path) -> ProbeResult {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path).await.unwrap_or_default()
        } else {
            Default::default()
        };

        let Some((session_id, hist)) = history::find_session_for_dir(&history_map, dir) else {
            return ProbeResult::no_session();
        };

        let path = self.session_path(session_id);
        if !path.exists() {
            return ProbeResult::no_session();
        }

        let log = match log_parser::read_log_tail(&path, 200).await {
            Ok(l) => l,
            Err(_) => return ProbeResult::no_session(),
        };
        let meta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(_) => return ProbeResult::no_session(),
        };
        let fs = FsSignals {
            sampled_at: SystemTime::now(),
            log_mtime: meta.modified().unwrap_or(SystemTime::now()),
            process_holds_file: process::file_is_held_open(&path).await,
        };
        let state = classifier::classify(&log, &fs);

        ProbeResult {
            state,
            session_id: Some(session_id.clone()),
            last_activity: log.last_line_ts,
            last_user_message: hist.last_display().map(str::to_string),
        }
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path).await.unwrap_or_default()
        } else {
            return vec![];
        };
        let Some((session_id, _)) = history::find_session_for_dir(&history_map, dir) else {
            return vec![];
        };
        let path = self.session_path(session_id);
        let log = match log_parser::read_log_tail(&path, 100).await {
            Ok(l) => l,
            Err(_) => return vec![],
        };
        log.tail.iter().map(|l| {
            let ts = l.timestamp.format("%H:%M:%S").to_string();
            let level = match l.level {
                vigil_core::LogLevel::Debug => "DBG",
                vigil_core::LogLevel::Info  => "INF",
                vigil_core::LogLevel::Warn  => "WRN",
                vigil_core::LogLevel::Error => "ERR",
            };
            let comp = l.component.as_deref().unwrap_or("");
            if comp.is_empty() {
                format!("{ts} {level}  {}", l.message)
            } else {
                format!("{ts} {level}  [{comp}] {}", l.message)
            }
        }).collect()
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("claude");
        cmd.arg("--resume").arg(&session_id.0);
        cmd.env_remove("CLAUDECODE");
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("claude");
        cmd.current_dir(dir);
        cmd
    }

    async fn send_message(
        &self,
        dir: &Path,
        _session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        let history_map = history::load_history(&self.history_path).await?;
        let Some((session_id, _)) = history::find_session_for_dir(&history_map, dir) else {
            return Err(VigilError::NotSupported("no session found for directory".into()));
        };
        let log_path = self.session_path(session_id);

        let pids = vigil_core::process::get_pids_holding_file(&log_path).await;
        let pid = pids
            .first()
            .copied()
            .ok_or_else(|| VigilError::NotSupported("no process holds session file".into()))?;

        vigil_core::process::send_to_tty(pid, msg).await
    }
}
