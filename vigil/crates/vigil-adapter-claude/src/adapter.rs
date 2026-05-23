use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, FsSignals, LogEvent, ProbeResult, SessionId, VigilError};

use crate::{classifier, history, log_parser, process, session_parser};

#[derive(Clone)]
pub struct ClaudeCodeAdapter {
    debug_dir: PathBuf,
    projects_dir: PathBuf,
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
            projects_dir: claude_dir.join("projects"),
            history_path: claude_dir.join("history.jsonl"),
        })
    }

    pub fn with_paths(debug_dir: PathBuf, history_path: PathBuf) -> Self {
        let projects_dir = debug_dir.parent()
            .unwrap_or(&debug_dir)
            .join("projects");
        Self { debug_dir, projects_dir, history_path }
    }

    fn session_path(&self, id: &SessionId) -> PathBuf {
        self.debug_dir.join(format!("{}.txt", id.0))
    }
}

/// Scan `projects_dir` for `{session_id}.jsonl` in any immediate subdirectory.
async fn find_session_jsonl(projects_dir: &Path, session_id: &SessionId) -> Option<PathBuf> {
    let filename = format!("{}.jsonl", session_id.0);
    let mut dir = tokio::fs::read_dir(projects_dir).await.ok()?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let candidate = entry.path().join(&filename);
        if tokio::fs::metadata(&candidate).await.is_ok() {
            return Some(candidate);
        }
    }
    None
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

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path).await.unwrap_or_default()
        } else {
            return vec![];
        };
        let Some((session_id, _)) = history::find_session_for_dir(&history_map, dir) else {
            return vec![];
        };
        let Some(jsonl_path) = find_session_jsonl(&self.projects_dir, session_id).await else {
            return vec![];
        };
        let content = match tokio::fs::read_to_string(&jsonl_path).await {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        session_parser::parse_session_jsonl(&content)
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
        cmd.arg("--dangerously-skip-permissions");
        cmd.arg("--debug");
        cmd.arg("--resume").arg(&session_id.0);
        cmd.env_remove("CLAUDECODE");
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("claude");
        cmd.arg("--dangerously-skip-permissions");
        cmd.arg("--debug");
        cmd.current_dir(dir);
        cmd
    }

    async fn start_with_message(
        &self,
        dir: &Path,
        msg: &str,
    ) -> Result<(), VigilError> {
        tokio::process::Command::new("claude")
            .arg("--dangerously-skip-permissions")
            .arg("--debug")
            .arg("--print").arg(msg)
            .env_remove("CLAUDECODE")
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("claude spawn failed: {e}")))?;
        Ok(())
    }

    async fn send_message(
        &self,
        dir: &Path,
        session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        // Do not try to type into an existing Claude TTY. In Vigil there often is no
        // attached interactive TTY, and the process holding the debug log may be an
        // internal child process. Resume the conversation in print mode instead,
        // matching the Pi adapter's fire-and-forget behavior.
        tokio::process::Command::new("claude")
            .arg("--dangerously-skip-permissions")
            .arg("--debug")
            .arg("--resume").arg(&session_id.0)
            .arg("--print").arg(msg)
            .env_remove("CLAUDECODE")
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("claude spawn failed: {e}")))?;
        Ok(())
    }
}
