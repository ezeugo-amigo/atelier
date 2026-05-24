use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use vigil_core::{
    AgentAdapter, AgentKind, FsSignals, LogEvent, ProbeResult, SessionId, VigilError,
};

use crate::{classifier, history, log_parser, process, session_parser};

#[derive(Clone)]
pub struct ClaudeCodeAdapter {
    debug_dir: PathBuf,
    projects_dir: PathBuf,
    history_path: PathBuf,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Result<Self, VigilError> {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| VigilError::ProcessProbe("cannot determine home directory".into()))?;
        let claude_dir = base.home_dir().join(".claude");
        Ok(Self {
            debug_dir: claude_dir.join("debug"),
            projects_dir: claude_dir.join("projects"),
            history_path: claude_dir.join("history.jsonl"),
        })
    }

    pub fn with_paths(debug_dir: PathBuf, history_path: PathBuf) -> Self {
        let projects_dir = debug_dir.parent().unwrap_or(&debug_dir).join("projects");
        Self {
            debug_dir,
            projects_dir,
            history_path,
        }
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

#[derive(Debug, Clone)]
struct JsonlSession {
    id: SessionId,
    path: PathBuf,
    last_ts: Option<DateTime<Utc>>,
    last_user_message: Option<String>,
}

fn claude_project_dir_name(dir: &Path) -> String {
    dir.display()
        .to_string()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }

    let text = content
        .as_array()?
        .iter()
        .filter(|block| block["type"].as_str() == Some("text"))
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");

    (!text.trim().is_empty()).then_some(text)
}

async fn parse_jsonl_session_for_dir(path: PathBuf, dir: &Path) -> Option<JsonlSession> {
    let content = tokio::fs::read_to_string(&path).await.ok()?;
    let mut id = None;
    let mut matches_dir = false;
    let mut last_ts: Option<DateTime<Utc>> = None;
    let mut last_user_message = None;

    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;
        if val["cwd"].as_str().map(Path::new) == Some(dir) {
            matches_dir = true;
        }
        if id.is_none() {
            id = val["sessionId"].as_str().map(|s| SessionId(s.to_string()));
        }
        if let Some(ts) = val["timestamp"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
        {
            last_ts = Some(last_ts.map_or(ts, |prev| prev.max(ts)));
        }
        if val["type"].as_str() == Some("user") && val["message"]["role"].as_str() == Some("user") {
            if let Some(text) = extract_user_text(&val["message"]["content"]) {
                last_user_message = Some(text);
            }
        }
    }

    Some(JsonlSession {
        id: id?,
        path,
        last_ts,
        last_user_message: matches_dir.then_some(last_user_message).flatten(),
    })
    .filter(|_| matches_dir)
}

/// `claude --print` does not reliably update `~/.claude/history.jsonl`, so
/// discover the newest session for a worktree directly from project JSONL files.
async fn find_jsonl_session_for_dir(projects_dir: &Path, dir: &Path) -> Option<JsonlSession> {
    let project_dir = projects_dir.join(claude_project_dir_name(dir));
    let mut best = None;

    let mut entries = tokio::fs::read_dir(project_dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(session) = parse_jsonl_session_for_dir(path, dir).await else {
            continue;
        };
        let replace = best
            .as_ref()
            .is_none_or(|current: &JsonlSession| session.last_ts > current.last_ts);
        if replace {
            best = Some(session);
        }
    }

    best
}

#[async_trait::async_trait]
impl AgentAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    /// Probe the container at `dir`: find the most recent Claude session for that directory,
    /// read its debug log, and classify the state.
    async fn probe(&self, dir: &Path) -> ProbeResult {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path)
                .await
                .unwrap_or_default()
        } else {
            Default::default()
        };

        let (session_id, last_user_message) = match history::find_session_for_dir(&history_map, dir)
        {
            Some((session_id, hist)) => {
                (session_id.clone(), hist.last_display().map(str::to_string))
            }
            None => match find_jsonl_session_for_dir(&self.projects_dir, dir).await {
                Some(session) => (session.id, session.last_user_message),
                None => return ProbeResult::no_session(),
            },
        };

        let path = self.session_path(&session_id);
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
            session_id: Some(session_id),
            last_activity: log.last_line_ts,
            last_user_message,
        }
    }

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path)
                .await
                .unwrap_or_default()
        } else {
            Default::default()
        };
        let jsonl_path = match history::find_session_for_dir(&history_map, dir) {
            Some((session_id, _)) => match find_session_jsonl(&self.projects_dir, session_id).await
            {
                Some(path) => path,
                None => return vec![],
            },
            None => match find_jsonl_session_for_dir(&self.projects_dir, dir).await {
                Some(session) => session.path,
                None => return vec![],
            },
        };
        let content = match tokio::fs::read_to_string(&jsonl_path).await {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        session_parser::parse_session_jsonl(&content)
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let history_map = if self.history_path.exists() {
            history::load_history(&self.history_path)
                .await
                .unwrap_or_default()
        } else {
            Default::default()
        };
        let session_id = match history::find_session_for_dir(&history_map, dir) {
            Some((session_id, _)) => session_id.clone(),
            None => match find_jsonl_session_for_dir(&self.projects_dir, dir).await {
                Some(session) => session.id,
                None => return vec![],
            },
        };
        let path = self.session_path(&session_id);
        let log = match log_parser::read_log_tail(&path, 100).await {
            Ok(l) => l,
            Err(_) => return vec![],
        };
        log.tail
            .iter()
            .map(|l| {
                let ts = l.timestamp.format("%H:%M:%S").to_string();
                let level = match l.level {
                    vigil_core::LogLevel::Debug => "DBG",
                    vigil_core::LogLevel::Info => "INF",
                    vigil_core::LogLevel::Warn => "WRN",
                    vigil_core::LogLevel::Error => "ERR",
                };
                let comp = l.component.as_deref().unwrap_or("");
                if comp.is_empty() {
                    format!("{ts} {level}  {}", l.message)
                } else {
                    format!("{ts} {level}  [{comp}] {}", l.message)
                }
            })
            .collect()
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

    async fn start_with_message(&self, dir: &Path, msg: &str) -> Result<(), VigilError> {
        tokio::process::Command::new("claude")
            .arg("--dangerously-skip-permissions")
            .arg("--debug")
            .arg("--print")
            .arg(msg)
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
            .arg("--resume")
            .arg(&session_id.0)
            .arg("--print")
            .arg(msg)
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
