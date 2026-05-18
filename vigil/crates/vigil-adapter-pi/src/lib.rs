//! Pi adapter for Vigil.
//!
//! Session files: ~/.pi/agent/sessions/<encoded-path>/<ISO8601>_<uuid>.jsonl
//! Path encoding: /Users/foo/bar → --Users-foo-bar--
//!
//! State is derived from the last message in the most recent session JSONL
//! combined with how recently the file was written (mtime proxy for activity).

pub mod classifier;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vigil_core::{AgentAdapter, AgentKind, ProbeResult, SessionId};

pub struct PiAdapter;

#[async_trait::async_trait]
impl AgentAdapter for PiAdapter {
    fn kind(&self) -> AgentKind { AgentKind::Pi }

    async fn probe(&self, dir: &Path) -> ProbeResult {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else {
            return ProbeResult::no_session();
        };

        let session_id = session_id_from_path(&latest);

        let secs_since_write = latest.metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(f64::MAX);

        let last_activity = latest.metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap_or_default());

        let content = tokio::fs::read_to_string(&latest).await.unwrap_or_default();
        let last_msg = classifier::parse_last_message(&content);
        let state = classifier::classify(last_msg, secs_since_write);

        // Extract last user message for display
        let last_user_message = last_user_message(&content);

        ProbeResult { state, session_id, last_activity, last_user_message }
    }

    async fn recent_log(&self, dir: &Path) -> Vec<String> {
        let session_dir = sessions_dir().join(encode_path(dir));
        let Some(latest) = find_latest(&session_dir) else { return vec![]; };
        let Ok(content) = tokio::fs::read_to_string(&latest).await else { return vec![]; };

        let lines: Vec<&str> = content.lines().collect();
        let tail = if lines.len() > 80 { &lines[lines.len() - 80..] } else { &lines };

        tail.iter()
            .filter_map(|line| {
                let val: serde_json::Value = serde_json::from_str(line).ok()?;
                format_message(&val)
            })
            .collect()
    }

    fn attach_command(&self, session_id: &SessionId, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.arg("--session").arg(&session_id.0);
        cmd.current_dir(dir);
        cmd
    }

    fn launch_command(&self, dir: &Path) -> std::process::Command {
        let mut cmd = std::process::Command::new("pi");
        cmd.current_dir(dir);
        cmd
    }
}

// ── Path / session helpers ────────────────────────────────────────────────────

/// `/Users/foo/bar` → `--Users-foo-bar--`
fn encode_path(dir: &Path) -> String {
    let s = dir.display().to_string();
    format!("--{}--", s.trim_start_matches('/').replace('/', "-"))
}

fn sessions_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".pi").join("agent").join("sessions"))
        .unwrap_or_else(|| PathBuf::from("~/.pi/agent/sessions"))
}

/// Return the most recent JSONL file in a session directory (alphabetical = chronological).
fn find_latest(session_dir: &Path) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(session_dir).ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "jsonl"))
        .collect();
    files.sort();
    files.into_iter().last()
}

/// Extract the UUID from `{ISO8601}_{uuid}.jsonl`.
fn session_id_from_path(path: &Path) -> Option<SessionId> {
    let stem = path.file_stem()?.to_str()?;
    let uuid = stem.splitn(2, '_').nth(1)?;
    Some(SessionId(uuid.to_string()))
}

/// Find the last user message text in the session for display.
fn last_user_message(content: &str) -> Option<String> {
    content.lines().rev()
        .find_map(|line| {
            let val: serde_json::Value = serde_json::from_str(line).ok()?;
            if val["message"]["role"].as_str() != Some("user") { return None; }
            val["message"]["content"].as_array()?
                .iter()
                .find(|c| c["type"] == "text")
                .and_then(|c| c["text"].as_str())
                .map(|s| s.chars().take(120).collect())
        })
}

// ── Message formatting (for log overlay) ─────────────────────────────────────

fn format_message(val: &serde_json::Value) -> Option<String> {
    if val["type"].as_str()? != "message" { return None; }
    let msg = &val["message"];
    match msg["role"].as_str()? {
        "user" => {
            let text = msg["content"].as_array()?
                .iter().find(|c| c["type"] == "text")?["text"].as_str()?;
            Some(format!("YOU   {}", text.chars().take(100).collect::<String>()))
        }
        "assistant" => {
            let content = msg["content"].as_array()?;
            for item in content {
                match item["type"].as_str() {
                    Some("text") => {
                        let text = item["text"].as_str().unwrap_or("");
                        return Some(format!("PI    {}", text.chars().take(100).collect::<String>()));
                    }
                    Some("toolCall") => {
                        let name = item["name"].as_str().unwrap_or("?");
                        let arg_hint = item["arguments"].as_object()
                            .and_then(|o| o.values().next())
                            .and_then(|v| v.as_str())
                            .map(|s| s.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        return Some(format!("TOOL  {name}({arg_hint})"));
                    }
                    _ => {}
                }
            }
            None
        }
        "toolResult" => {
            let name = msg["toolName"].as_str().unwrap_or("?");
            let snippet = msg["content"].as_array()
                .and_then(|a| a.first())
                .and_then(|c| c["text"].as_str())
                .map(|s| s.chars().take(80).collect::<String>())
                .unwrap_or_default();
            Some(format!("RSLT  {name} → {snippet}"))
        }
        _ => None,
    }
}
