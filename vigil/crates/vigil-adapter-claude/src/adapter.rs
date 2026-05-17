use std::path::PathBuf;
use std::time::SystemTime;
use vigil_core::{AgentAdapter, AgentKind, FsSignals, Session, SessionId, SessionLog, SessionState, VigilError};

use crate::{classifier, discover, history, log_parser, process};

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

    /// Returns raw (log tail, fs signals, state) without loading history — fast path for watchers.
    pub async fn sample_raw(
        &self,
        id: &SessionId,
    ) -> Result<(SessionLog, FsSignals, SessionState), VigilError> {
        let path = self.session_path(id);
        if !path.exists() {
            return Err(VigilError::SessionNotFound(id.0.clone()));
        }
        let log = log_parser::read_log_tail(&path, 200).await?;
        let meta = tokio::fs::metadata(&path).await?;
        let log_mtime = meta.modified()?;
        let sampled_at = SystemTime::now();
        let held = process::file_is_held_open(&path).await;
        let fs = FsSignals { sampled_at, log_mtime, process_holds_file: held };
        let state = classifier::classify(&log, &fs);
        Ok((log, fs, state))
    }
}

#[async_trait::async_trait]
impl AgentAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn roots(&self) -> Vec<PathBuf> {
        vec![self.debug_dir.clone()]
    }

    async fn discover(&self) -> Result<Vec<SessionId>, VigilError> {
        discover::discover_sessions(&self.debug_dir).await
    }

    async fn read(&self, id: &SessionId) -> Result<Session, VigilError> {
        let path = self.session_path(id);

        if !path.exists() {
            return Err(VigilError::SessionNotFound(id.0.clone()));
        }

        // Read log tail (200 lines)
        let log = log_parser::read_log_tail(&path, 200).await?;

        // Sample filesystem signals
        let meta = tokio::fs::metadata(&path).await?;
        let log_mtime = meta.modified()?;
        let sampled_at = SystemTime::now();
        let held = process::file_is_held_open(&path).await;

        let fs = FsSignals {
            sampled_at,
            log_mtime,
            process_holds_file: held,
        };

        // Classify state
        let state = self.classify(&log, &fs);

        // Load history (best-effort)
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path).await.unwrap_or_default()
        } else {
            Default::default()
        };

        let session_history = history_map.get(id);
        let project_dir = session_history.and_then(|h| h.project_dir());
        let started_at = session_history.and_then(|h| h.first_timestamp());
        let last_user_message = session_history
            .and_then(|h| h.last_display())
            .map(|s| s.to_string());
        let last_activity = log.last_line_ts;

        Ok(Session {
            id: id.clone(),
            kind: AgentKind::ClaudeCode,
            project_dir,
            started_at,
            last_activity,
            last_user_message,
            state,
        })
    }

    fn classify(&self, log: &SessionLog, fs: &FsSignals) -> SessionState {
        classifier::classify(log, fs)
    }

    fn attach_command(&self, id: &SessionId) -> std::process::Command {
        let mut cmd = std::process::Command::new("claude");
        cmd.arg("--resume").arg(&id.0);
        cmd
    }
}
