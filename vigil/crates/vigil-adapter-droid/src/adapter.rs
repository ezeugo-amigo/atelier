use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use vigil_core::{AgentAdapter, AgentKind, FsSignals, LogEvent, ProbeResult, SessionId, VigilError};

use crate::{classifier, session_parser};

#[derive(Clone)]
pub struct DroidAdapter {
    sessions_dir: PathBuf, // ~/.factory/sessions
}

impl DroidAdapter {
    pub fn new() -> Result<Self, VigilError> {
        let base = directories::BaseDirs::new().ok_or_else(|| {
            VigilError::ProcessProbe("cannot determine home directory".into())
        })?;
        let factory_dir = base.home_dir().join(".factory");
        Ok(Self {
            sessions_dir: factory_dir.join("sessions"),
        })
    }
}

/// Convert a path to droid's project directory name: replace `/` and `.` with `-`.
/// e.g. `/Users/foo/bar.baz` → `-Users-foo-bar-baz`
fn droid_project_dir_name(dir: &Path) -> String {
    dir.display()
        .to_string()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

struct DroidSession {
    id: SessionId,
    path: PathBuf,
    last_ts: Option<DateTime<Utc>>,
    last_user_message: Option<String>,
}

fn extract_user_text(content: &serde_json::Value) -> Option<String> {
    if let Some(s) = content.as_str() {
        let s = s.trim();
        return if s.is_empty() {
            None
        } else {
            Some(s.chars().take(2000).collect())
        };
    }
    let text = content
        .as_array()?
        .iter()
        .filter(|b| b["type"].as_str() == Some("text"))
        .filter_map(|b| b["text"].as_str())
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text.chars().take(2000).collect())
}

/// Find the most recent droid session for `dir` by scanning the project's sessions subdirectory.
async fn find_session_for_dir(sessions_dir: &Path, dir: &Path) -> Option<DroidSession> {
    let project_dir = sessions_dir.join(droid_project_dir_name(dir));
    let mut entries = tokio::fs::read_dir(&project_dir).await.ok()?;
    let mut best: Option<DroidSession> = None;

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        // First line is always `session_start` — verify cwd and extract id.
        let first_line = content.lines().next().unwrap_or("").trim();
        let Ok(start) = serde_json::from_str::<serde_json::Value>(first_line) else {
            continue;
        };
        if start["type"].as_str() != Some("session_start") {
            continue;
        }
        if start["cwd"].as_str().map(Path::new) != Some(dir) {
            continue;
        }
        let id = match start["id"].as_str() {
            Some(id) => SessionId(id.to_string()),
            None => continue,
        };

        let mut last_ts: Option<DateTime<Utc>> = None;
        let mut last_user_message: Option<String> = None;

        for line in content.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if val["type"].as_str() != Some("message") {
                continue;
            }
            if val["visibility"].as_str() == Some("llm_only") {
                continue;
            }

            if let Some(ts) = val["timestamp"]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
            {
                last_ts = Some(last_ts.map_or(ts, |prev| prev.max(ts)));
            }

            if val["message"]["role"].as_str() == Some("user") {
                let content_val = &val["message"]["content"];
                let is_only_tool_results = content_val
                    .as_array()
                    .map(|arr| {
                        !arr.is_empty()
                            && arr
                                .iter()
                                .all(|b| b["type"].as_str() == Some("tool_result"))
                    })
                    .unwrap_or(false);
                if !is_only_tool_results {
                    if let Some(text) = extract_user_text(content_val) {
                        last_user_message = Some(text);
                    }
                }
            }
        }

        let session = DroidSession {
            id,
            path,
            last_ts,
            last_user_message,
        };
        let replace = best
            .as_ref()
            .is_none_or(|cur: &DroidSession| session.last_ts > cur.last_ts);
        if replace {
            best = Some(session);
        }
    }

    best
}

#[async_trait::async_trait]
impl AgentAdapter for DroidAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Droid
    }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let Some(session) = find_session_for_dir(&self.sessions_dir, dir).await else {
            return ProbeResult::no_session();
        };

        let meta = match tokio::fs::metadata(&session.path).await {
            Ok(m) => m,
            Err(_) => return ProbeResult::no_session(),
        };
        let content = match tokio::fs::read_to_string(&session.path).await {
            Ok(c) => c,
            Err(_) => return ProbeResult::no_session(),
        };

        // Droid doesn't hold the session JSONL open between turns, so process_holds_file
        // is not a useful signal here — rely on mtime + last message role instead.
        let fs = FsSignals {
            sampled_at: SystemTime::now(),
            log_mtime: meta.modified().unwrap_or(SystemTime::now()),
            process_holds_file: None,
        };

        let state = classifier::classify(&content, &fs);

        ProbeResult {
            state,
            session_id: Some(session.id),
            last_activity: session.last_ts,
            last_user_message: session.last_user_message,
        }
    }

    async fn recent_log_events(&self, dir: &Path) -> Vec<LogEvent> {
        let Some(session) = find_session_for_dir(&self.sessions_dir, dir).await else {
            return vec![];
        };
        let content = match tokio::fs::read_to_string(&session.path).await {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        session_parser::parse_session_jsonl(&content)
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("droid");
        cmd.arg("--resume").arg(&session_id.0);
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("droid");
        cmd.current_dir(dir);
        cmd
    }

    async fn send_message(
        &self,
        dir: &Path,
        session_id: &SessionId,
        msg: &str,
    ) -> Result<(), VigilError> {
        tokio::process::Command::new("droid")
            .arg("exec")
            .arg("--session-id")
            .arg(&session_id.0)
            .arg("--skip-permissions-unsafe")
            .arg(msg)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("droid spawn failed: {e}")))?;
        Ok(())
    }

    async fn start_with_message(&self, dir: &Path, msg: &str) -> Result<(), VigilError> {
        tokio::process::Command::new("droid")
            .arg("exec")
            .arg("--skip-permissions-unsafe")
            .arg(msg)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VigilError::ProcessProbe(format!("droid spawn failed: {e}")))?;
        Ok(())
    }
}
